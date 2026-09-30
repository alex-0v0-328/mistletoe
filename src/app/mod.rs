//! 窗口、鼠标输入和主循环：把 state 和 render 接起来。
//! 不写渲染细节，也不写状态逻辑；只负责 事件 → state → render 的搬运和空闲调度。

use std::sync::Arc;
use std::time::Instant;

use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

use crate::render::Renderer;
use crate::state::{Pointer, State};

/// 创建事件循环并一直跑到窗口关闭。
pub fn run() -> Result<(), String> {
    let event_loop = EventLoop::new().map_err(|e| format!("创建事件循环失败：{e}"))?;
    // 默认睡眠，只有 request_redraw 或输入事件才会唤醒
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = App {
        start: Instant::now(),
        state: State::default(),
        view: None,
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
            .with_inner_size(LogicalSize::new(480.0, 480.0));
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .map_err(|e| format!("创建窗口失败：{e}"))?,
        );
        let size = window.inner_size();
        let renderer = Renderer::new(window.clone().into(), size.width, size.height)?;
        Ok(View { window, renderer })
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
                view.window.request_redraw();
            }
            WindowEvent::CursorMoved { position, .. } => {
                let pointer = normalize_pointer(position, view.window.inner_size());
                // 只有跟随目标变了才重绘；指针不动时不产生任何帧
                if pointer.is_some() && self.state.set_pointer(pointer) {
                    view.window.request_redraw();
                }
            }
            WindowEvent::CursorLeft { .. } => {
                if self.state.set_pointer(None) {
                    view.window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                let size = view.window.inner_size();
                // 最小化时不渲染；恢复时 Resized 会再请求重绘
                if view.window.is_minimized() == Some(true) || size.width == 0 || size.height == 0 {
                    return;
                }
                let snapshot = self.state.tick(self.start.elapsed().as_secs_f64());
                view.renderer.draw(&snapshot);
                // 还在动就要下一帧；AutoVsync 让 present 按刷新率节流
                if snapshot.animating {
                    view.window.request_redraw();
                }
            }
            _ => {}
        }
    }
}

/// 像素坐标（原点左上、y 向下）→ 以窗口中心为原点、y 向上、边缘为 ±1 的坐标。窗口尺寸为 0 时返回 None。
fn normalize_pointer(
    position: PhysicalPosition<f64>,
    size: winit::dpi::PhysicalSize<u32>,
) -> Option<Pointer> {
    if size.width == 0 || size.height == 0 {
        return None;
    }
    Some(Pointer {
        x: (position.x / size.width as f64 * 2.0 - 1.0) as f32,
        y: (1.0 - position.y / size.height as f64 * 2.0) as f32,
    })
}
