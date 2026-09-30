//! 窗口、鼠标输入和主循环：把 state 和 render 接起来。
//! 不写渲染细节，也不写状态逻辑；只负责 事件 → state → render 的搬运和空闲调度。

mod cursor;

use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::{DeviceEvent, DeviceId, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, DeviceEvents, EventLoop};
use winit::window::{Window, WindowId};

use crate::render::Renderer;
use crate::state::{Pointer, State, Tables};

/// 动画最高 60 fps：高刷屏上也不按刷新率出帧，省电，慢设备也跑得动。
const FRAME_INTERVAL: Duration = Duration::from_nanos(1_000_000_000 / 60);
/// 初始窗口大小（逻辑像素）。球的大小按窗口短边等比例缩放。
const WINDOW_SIZE: f64 = 360.0;

/// 创建事件循环并一直跑到窗口关闭。
pub fn run() -> Result<(), String> {
    let event_loop = EventLoop::new().map_err(|e| format!("创建事件循环失败：{e}"))?;
    // 默认睡眠，只有输入事件或排好的下一帧才会唤醒
    event_loop.set_control_flow(ControlFlow::Wait);
    // 窗口在后台时也接收原始鼠标移动，这样鼠标在窗口外也能跟随；鼠标不动就没有事件，不影响空闲
    event_loop.listen_device_events(DeviceEvents::Always);
    // 第 4 步改成从 --data 目录读取并热重载；现在用编进程序的默认数据
    let tables = Tables::parse(
        include_str!("../../data/presets.json"),
        include_str!("../../data/tags.json"),
    )?;
    // 随机眨眼的种子：每次启动不同就行
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(1, |d| d.as_nanos() as u64);
    let mut app = App {
        start: Instant::now(),
        state: State::new(tables, seed, 0.0),
        view: None,
        last_frame: None,
        frame_due: None,
        wakeup: None,
        error: None,
    };
    event_loop
        .run_app(&mut app)
        .map_err(|e| format!("事件循环异常退出：{e}"))?;
    app.error.map_or(Ok(()), Err)
}

struct App {
    /// state 里的时间都是“从 start 起的秒数”
    start: Instant,
    state: State,
    /// 窗口在 resumed 里才创建
    view: Option<View>,
    /// 上一帧开始画的时间
    last_frame: Option<Instant>,
    /// 已排好的下一帧的时间；None = 没有要画的
    frame_due: Option<Instant>,
    /// 空闲时要醒来的时刻（下一次自动眨眼）；None = 一直睡到有输入
    wakeup: Option<Instant>,
    /// 启动阶段的致命错误，退出事件循环后由 run 返回
    error: Option<String>,
}

struct View {
    window: Arc<Window>,
    renderer: Renderer,
}

impl App {
    fn create_view(event_loop: &ActiveEventLoop) -> Result<View, String> {
        let attributes = Window::default_attributes()
            .with_title("Mistletoe")
            .with_inner_size(LogicalSize::new(WINDOW_SIZE, WINDOW_SIZE));
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .map_err(|e| format!("创建窗口失败：{e}"))?,
        );
        let size = window.inner_size();
        let renderer = Renderer::new(window.clone().into(), size.width, size.height)?;
        Ok(View { window, renderer })
    }

    /// 排一帧动画：离上一帧不足 FRAME_INTERVAL 就往后推，已经排过就不重复排。
    /// 真正的 request_redraw 在 about_to_wait 里按时发出。
    fn schedule_frame(&mut self) {
        if self.frame_due.is_some() {
            return;
        }
        let now = Instant::now();
        let earliest = self.last_frame.map_or(now, |last| last + FRAME_INTERVAL);
        self.frame_due = Some(earliest.max(now));
    }

    /// 读全局鼠标位置，换算成相对窗口中心的坐标交给 state。
    /// 只有跟随目标变了才排帧；鼠标不动时不产生任何帧。
    fn follow_cursor(&mut self) {
        let Some(view) = &self.view else {
            return;
        };
        let Some((x, y)) = cursor::screen_position() else {
            return;
        };
        // 客户区左上角的屏幕坐标：鼠标位置减去它，就是相对窗口的像素坐标
        let Ok(origin) = view.window.inner_position() else {
            return;
        };
        let Some(pointer) = to_pointer(x - origin.x, y - origin.y, view.window.inner_size()) else {
            return;
        };
        if self.state.set_pointer(pointer) {
            self.schedule_frame();
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.view.is_some() {
            return;
        }
        match Self::create_view(event_loop) {
            Ok(view) => {
                view.window.request_redraw();
                self.view = Some(view);
                // 一打开就看向鼠标，不用等它先动一下
                self.follow_cursor();
            }
            Err(e) => {
                self.error = Some(e);
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(view) = self.view.as_mut() else {
            return;
        };
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                view.renderer.resize(size.width, size.height);
                // 尺寸变了要立刻重画，不走 60 fps 节流，否则拖动边框时画面会拉伸
                view.window.request_redraw();
                // 窗口中心变了，鼠标相对球的位置也跟着变
                self.follow_cursor();
            }
            WindowEvent::Moved(_) | WindowEvent::CursorMoved { .. } => self.follow_cursor(),
            WindowEvent::RedrawRequested => {
                let size = view.window.inner_size();
                // 最小化时不渲染；恢复时 Resized 会再请求重绘
                if view.window.is_minimized() == Some(true) || size.width == 0 || size.height == 0 {
                    return;
                }
                self.last_frame = Some(Instant::now());
                let snapshot = self.state.tick(self.start.elapsed().as_secs_f64());
                view.renderer.draw(&snapshot);
                // 还在动就排下一帧（最高 60 fps）；停了就睡到下一次眨眼或下一个输入
                self.wakeup = None;
                if snapshot.animating {
                    self.schedule_frame();
                } else if let Some(t) = snapshot.next_wakeup {
                    self.wakeup = Duration::try_from_secs_f64(t).ok().map(|d| self.start + d);
                }
            }
            _ => {}
        }
    }

    fn device_event(&mut self, _event_loop: &ActiveEventLoop, _id: DeviceId, event: DeviceEvent) {
        // 原始鼠标移动只当作“鼠标动了”的信号，位置以 GetCursorPos 为准（原始增量不含系统的指针加速）
        if let DeviceEvent::MouseMotion { .. } = event {
            self.follow_cursor();
        }
    }

    /// 每轮事件处理完、准备睡眠前调用：到点了就发出重绘，没到点就睡到最早要做事的那一刻。
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        // 自动眨眼到点了：排一帧，tick 会开始眨眼
        if self.wakeup.is_some_and(|at| now >= at) {
            self.wakeup = None;
            self.schedule_frame();
        }
        if let (Some(view), Some(due)) = (&self.view, self.frame_due)
            && now >= due
        {
            self.frame_due = None;
            view.window.request_redraw();
        }
        let next = match (self.frame_due, self.wakeup) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        event_loop.set_control_flow(next.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
    }
}

/// 相对客户区左上角的像素坐标（y 向下，可以在窗口外）→ 以窗口中心为原点、y 向上、
/// 以半个窗口短边为单位的坐标。窗口尺寸为 0（最小化）时返回 None。
fn to_pointer(x: i32, y: i32, size: PhysicalSize<u32>) -> Option<Pointer> {
    let half = size.width.min(size.height) as f32 * 0.5;
    if half == 0.0 {
        return None;
    }
    Some(Pointer {
        x: (x as f32 - size.width as f32 * 0.5) / half,
        y: (size.height as f32 * 0.5 - y as f32) / half,
    })
}
