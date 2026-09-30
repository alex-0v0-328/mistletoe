//! wgpu 渲染：一个全屏三角形 + 片元着色器画出整个球。
//! 只读 state::Snapshot；不认识 api，也不认识 winit（窗口以 wgpu::SurfaceTarget 传进来）。

use crate::state::{Pose, Snapshot};

/// 球半径占窗口短边的比例：直径约为短边的 46%。
const RADIUS_RATIO: f32 = 0.228;
/// 描边宽度（物理像素），与窗口大小无关。
const OUTLINE_PX: f32 = 4.0;
/// Globals 的字节数：4 个 vec4<f32>。
const GLOBALS_SIZE: usize = 4 * 16;

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
            // 省电优先：有核显就用核显，空闲时几乎不占资源
            power_preference: wgpu::PowerPreference::LowPower,
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
        let globals = Globals::new(self.config.width, self.config.height, snapshot.pose);
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
    /// 宽、高（物理像素）、球半径（像素）、描边宽度（像素）
    screen: [f32; 4],
    /// 脸的基向量 U、V、F（视图空间），见 face_basis
    face_u: [f32; 4],
    face_v: [f32; 4],
    face_f: [f32; 4],
}

impl Globals {
    fn new(width: u32, height: u32, pose: Pose) -> Self {
        let (w, h) = (width as f32, height as f32);
        let [u, v, f] = face_basis(pose);
        Self {
            screen: [w, h, RADIUS_RATIO * w.min(h), OUTLINE_PX],
            face_u: [u[0], u[1], u[2], 0.0],
            face_v: [v[0], v[1], v[2], 0.0],
            face_f: [f[0], f[1], f[2], 0.0],
        }
    }

    /// 按字段顺序写成小端字节，布局与 WGSL 一致（全是 vec4<f32>，没有填充）。
    fn to_bytes(&self) -> [u8; GLOBALS_SIZE] {
        let mut out = [0u8; GLOBALS_SIZE];
        let fields = [self.screen, self.face_u, self.face_v, self.face_f];
        for (i, value) in fields.iter().flatten().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        out
    }
}

/// 由姿态求旋转矩阵 R = Ry(yaw) · Rx(pitch) 的三列：U = R·x̂，V = R·ŷ，F = R·ẑ。
/// 视图空间：x 向右、y 向上、+z 指向观众。yaw 为正时 F 偏向 +x，pitch 为正时 F 偏向 +y。
fn face_basis(pose: Pose) -> [[f32; 3]; 3] {
    let (sy, cy) = pose.yaw.sin_cos();
    let (sp, cp) = pose.pitch.sin_cos();
    [
        [cy, 0.0, -sy],
        [-sy * sp, cp, -cy * sp],
        [sy * cp, sp, cy * cp],
    ]
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
        });
        for (a, b) in [(u, v), (v, f), (f, u)] {
            assert!(dot(a, b).abs() < 1e-6);
        }
        for a in [u, v, f] {
            assert!((dot(a, a) - 1.0).abs() < 1e-6);
        }
        // yaw 为正 → 脸朝右；pitch 为负 → 脸朝下
        assert!(f[0] > 0.0 && f[1] < 0.0);
    }

    #[test]
    fn globals_bytes_follow_field_order() {
        let bytes = Globals::new(800, 600, Pose::default()).to_bytes();
        let read = |i: usize| f32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap());
        assert_eq!(read(0), 800.0);
        assert_eq!(read(1), 600.0);
        assert_eq!(read(2), RADIUS_RATIO * 600.0);
        assert_eq!(read(4), 1.0); // U = x̂
        assert_eq!(read(9), 1.0); // V = ŷ
        assert_eq!(read(14), 1.0); // F = ẑ
    }
}
