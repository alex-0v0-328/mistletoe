//! wgpu 渲染：一个全屏三角形 + 片元着色器画出整个球。
//! 只读 state::Snapshot；不认识 api，也不认识 winit（窗口以 wgpu::SurfaceTarget 传进来）。

use crate::state::{Eyes, MouthShape, Pose, Snapshot};

/// 球半径占窗口短边的比例：直径约为短边的 46%。
const RADIUS_RATIO: f32 = 0.228;
/// 外轮廓宽度：球半径的倍数（和原型一样随球一起缩放）
const OUTLINE: f32 = 0.04;
/// 眼神：球转过 1 弧度时，眼睛在脸空间里额外偏移多少（嘴偏移它的 0.6 倍，在着色器里算）
const LOOK_GAIN: f32 = 0.07;
/// Globals 的字节数：9 个 vec4<f32>。
const GLOBALS_SIZE: usize = 9 * 16;

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    globals_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

impl Renderer {
    /// 创建 GPU 设备、表面和管线。启动时调用一次，内部用 pollster 阻塞等待 wgpu 的异步请求。
    pub fn new(
        target: wgpu::SurfaceTarget<'static>,
        width: u32,
        height: u32,
    ) -> Result<Self, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::DX12,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let surface = instance
            .create_surface(target)
            .map_err(|e| format!("创建窗口表面失败：{e}"))?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            // 不排序：wgpu 直接用 DXGI 的第 0 块显卡，也就是接着主显示器的那块。
            // 这样画面不用跨显卡拷贝（跨显卡时实测偶尔出现横向残片）；普通笔记本上它就是核显，照样省电。
            power_preference: wgpu::PowerPreference::None,
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .map_err(|e| format!("找不到可用的显卡：{e}"))?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("mistletoe"),
            // 默认的 Performance 会按大块预留显存；核显的显存就是内存，实测进程多占约 200 MB
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            ..Default::default()
        }))
        .map_err(|e| format!("打开显卡设备失败：{e}"))?;

        let mut config = surface
            .get_default_config(&adapter, width.max(1), height.max(1))
            .ok_or("这块显卡不支持在此窗口上绘制")?;
        // 选非 sRGB 格式：调色板常量按显示字节值写，不经过 sRGB 编码。
        let formats = surface.get_capabilities(&adapter).formats;
        match formats.iter().find(|f| !f.is_srgb()) {
            Some(format) => config.format = *format,
            None => println!("警告：显卡只提供 sRGB 表面格式，灰度会比设计值偏亮"),
        }
        config.present_mode = wgpu::PresentMode::AutoVsync;
        surface.configure(&device, &config);

        let info = adapter.get_info();
        println!(
            "渲染器：{}（{:?}），表面格式 {:?}",
            info.name, info.backend, config.format
        );

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ball.wgsl"),
            source: wgpu::ShaderSource::Wgsl(include_str!("ball.wgsl").into()),
        });
        let globals_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: GLOBALS_SIZE as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals_buffer.as_entire_binding(),
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ball"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ball"),
            layout: Some(&layout),
            // 没有顶点缓冲：顶点着色器用 vertex_index 生成全屏三角形
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    // 不混合：像素只会是调色板里的颜色
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        Ok(Self {
            surface,
            device,
            queue,
            config,
            pipeline,
            globals_buffer,
            bind_group,
        })
    }

    /// 窗口尺寸变化（物理像素）。尺寸为 0（最小化）时忽略。
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }

    /// 按快照画一帧。拿不到交换链图像时跳过这一帧。
    pub fn draw(&mut self, snapshot: &Snapshot) {
        let Some(frame) = self.acquire_frame() else {
            return;
        };
        let globals = Globals::new(self.config.width, self.config.height, snapshot);
        self.queue
            .write_buffer(&self.globals_buffer, 0, &globals.to_bytes());

        let view = frame.texture.create_view(&Default::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ball"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit([encoder.finish()]);
        self.queue.present(frame);
    }

    /// 取下一张交换链图像。表面过期或丢失时重新配置并再试一次。
    fn acquire_frame(&mut self) -> Option<wgpu::SurfaceTexture> {
        for _ in 0..2 {
            match self.surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(frame) => return Some(frame),
                wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                    // 这一帧照常画，下一帧前重新配置
                    self.surface.configure(&self.device, &self.config);
                    return Some(frame);
                }
                wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                    return None;
                }
                wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                    self.surface.configure(&self.device, &self.config);
                }
                wgpu::CurrentSurfaceTexture::Validation => {
                    println!("错误：获取交换链图像时出现校验错误，跳过这一帧");
                    return None;
                }
            }
        }
        None
    }
}

/// 每帧上传的 uniform，对应 ball.wgsl 里的 Globals。
struct Globals {
    /// 宽、高（物理像素）、球半径（像素，已含换表情时的放大）、外轮廓宽度（球半径的倍数）
    screen: [f32; 4],
    /// 球心相对窗口中心的位移（像素，x 向右、y 向上），后两项未用
    ball: [f32; 4],
    /// 脸的基向量 U、V、F（视图空间），见 face_basis
    face_u: [f32; 4],
    face_v: [f32; 4],
    face_f: [f32; 4],
    /// 眼型编号、睁眼程度（1 睁开 .. 0 闭上）、嘴型编号、脸的竖直缩放（编号见 eye_code / mouth_code）
    expr: [f32; 4],
    /// 嘴宽、张开、弯曲，最后一项未用
    mouth: [f32; 4],
    /// 腮红、眼泪、汗、阴沉脸的显现程度
    overlay: [f32; 4],
    /// 鼻涕泡的显现程度、眼神偏移 x、y（脸空间），最后一项未用
    extra: [f32; 4],
}

impl Globals {
    fn new(width: u32, height: u32, snapshot: &Snapshot) -> Self {
        let (w, h) = (width as f32, height as f32);
        let radius = RADIUS_RATIO * w.min(h) * snapshot.scale;
        let pose = snapshot.pose;
        let [u, v, f] = face_basis(pose);
        let e = &snapshot.face.expression;
        Self {
            screen: [w, h, radius, OUTLINE],
            // Pose 的位移以球半径为单位
            ball: [pose.x * radius, pose.y * radius, 0.0, 0.0],
            face_u: [u[0], u[1], u[2], 0.0],
            face_v: [v[0], v[1], v[2], 0.0],
            face_f: [f[0], f[1], f[2], 0.0],
            expr: [
                eye_code(e.eyes),
                1.0 - snapshot.face.blink,
                mouth_code(e.mouth.shape),
                snapshot.face.squish,
            ],
            mouth: [e.mouth.width, e.mouth.open, e.mouth.curve, 0.0],
            overlay: [e.blush, e.tears, e.sweat, e.gloom],
            // 眼神跟着转动方向多偏一点，产生轻微的立体感
            extra: [e.bubble, pose.yaw * LOOK_GAIN, pose.pitch * LOOK_GAIN, 0.0],
        }
    }

    /// 按字段顺序写成小端字节，布局与 WGSL 一致（全是 vec4<f32>，没有填充）。
    fn to_bytes(&self) -> [u8; GLOBALS_SIZE] {
        let mut out = [0u8; GLOBALS_SIZE];
        let fields = [
            self.screen,
            self.ball,
            self.face_u,
            self.face_v,
            self.face_f,
            self.expr,
            self.mouth,
            self.overlay,
            self.extra,
        ];
        for (i, value) in fields.iter().flatten().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        out
    }
}

/// 眼型在着色器里的编号，必须和 ball.wgsl 的 eye_sd 对应
fn eye_code(eyes: Eyes) -> f32 {
    match eyes {
        Eyes::Dot => 0.0,
        Eyes::Smile => 1.0,
        Eyes::Closed => 2.0,
        Eyes::Squint => 3.0,
        Eyes::Wide => 4.0,
        Eyes::Sad => 5.0,
        Eyes::Annoyed => 6.0,
    }
}

/// 嘴型在着色器里的编号，必须和 ball.wgsl 的 mouth_sd 对应
fn mouth_code(shape: MouthShape) -> f32 {
    match shape {
        MouthShape::Line => 0.0,
        MouthShape::Cat => 1.0,
        MouthShape::Triangle => 2.0,
        MouthShape::Wavy => 3.0,
        MouthShape::Grin => 4.0,
    }
}

/// 由姿态求旋转矩阵 R = Ry(yaw) · Rx(pitch) · Rz(roll) 的三列：U = R·x̂，V = R·ŷ，F = R·ẑ。
/// 视图空间：x 向右、y 向上、+z 指向观众。yaw 为正时 F 偏向 +x，pitch 为正时 F 偏向 +y，
/// roll 为正时脸在自身平面里逆时针转。
fn face_basis(pose: Pose) -> [[f32; 3]; 3] {
    let (sy, cy) = pose.yaw.sin_cos();
    let (sp, cp) = pose.pitch.sin_cos();
    let (sr, cr) = pose.roll.sin_cos();
    // 先求 Ry · Rx 的三列
    let u0 = [cy, 0.0, -sy];
    let v0 = [-sy * sp, cp, -cy * sp];
    let f = [sy * cp, sp, cy * cp];
    // 再乘 Rz(roll)：只在 U、V 构成的脸平面里转，F 不变
    let u = [0, 1, 2].map(|i| cr * u0[i] + sr * v0[i]);
    let v = [0, 1, 2].map(|i| -sr * u0[i] + cr * v0[i]);
    [u, v, f]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
        a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
    }

    #[test]
    fn face_basis_is_orthonormal_and_points_where_expected() {
        let [u, v, f] = face_basis(Pose {
            yaw: 0.4,
            pitch: -0.3,
            roll: 0.2,
            ..Pose::default()
        });
        for (a, b) in [(u, v), (v, f), (f, u)] {
            assert!(dot(a, b).abs() < 1e-6);
        }
        for a in [u, v, f] {
            assert!((dot(a, a) - 1.0).abs() < 1e-6);
        }
        // yaw 为正 → 脸朝右；pitch 为负 → 脸朝下
        assert!(f[0] > 0.0 && f[1] < 0.0);
        // 只有 roll：脸的“右”转向上方（逆时针）
        let [u, _, _] = face_basis(Pose {
            roll: 0.3,
            ..Pose::default()
        });
        assert!(u[1] > 0.0);
    }

    #[test]
    fn globals_bytes_follow_field_order() {
        // 从真实状态拿一帧快照，再把每个字段改成不同的值，检查它们落在正确的位置
        let tables = crate::state::Tables::parse(
            include_str!("../../data/presets.json"),
            include_str!("../../data/tags.json"),
        )
        .unwrap();
        let mut snap = crate::state::State::new(tables, 1, 0.0).tick(0.0);
        snap.pose.y = 0.5;
        snap.pose.yaw = 0.5;
        snap.scale = 1.5;
        snap.face.blink = 0.75;
        snap.face.squish = 0.25;
        let e = &mut snap.face.expression;
        e.eyes = Eyes::Wide;
        e.mouth.shape = MouthShape::Grin;
        (e.mouth.width, e.mouth.open, e.mouth.curve) = (0.3, 0.4, -0.5);
        (e.blush, e.tears, e.sweat, e.gloom, e.bubble) = (0.1, 0.2, 0.3, 0.4, 0.5);
        let bytes = Globals::new(800, 600, &snap).to_bytes();
        let read = |i: usize| f32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap());
        let radius = RADIUS_RATIO * 600.0 * 1.5;
        assert_eq!(
            [read(0), read(1), read(2), read(3)],
            [800.0, 600.0, radius, OUTLINE]
        );
        assert_eq!(read(5), 0.5 * radius); // 球心上移半个半径
        assert_eq!(read(13), 1.0); // V = ŷ（只有 yaw，没有 pitch）
        let expr_and_after: Vec<f32> = (20..36).map(read).collect();
        let look_x = 0.5 * LOOK_GAIN;
        assert_eq!(
            expr_and_after,
            [
                4.0, 0.25, 4.0, 0.25, 0.3, 0.4, -0.5, 0.0, 0.1, 0.2, 0.3, 0.4, 0.5, look_x, 0.0,
                0.0
            ]
        );
    }
}
