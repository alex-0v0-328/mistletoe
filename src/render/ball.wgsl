// 球的着色器：全屏三角形 + 逐像素求光线与球的交点，画出球体、描边、硬明暗和脸。
// 不做任何半透明混合：每个输出像素都必须是下面调色板里的某一个颜色。

// 与 render/mod.rs 里的 Globals 逐字节对应；只用 vec4<f32>，避开 WGSL 对齐坑。
struct Globals {
    screen: vec4<f32>,  // x = 宽，y = 高（物理像素），z = 球半径（像素），w = 描边宽度（像素）
    ball: vec4<f32>,    // xy = 球心相对窗口中心的位移（像素，y 向上），zw 未用
    face_u: vec4<f32>,  // 脸的“右”方向（视图空间，旋转后的 x 轴）
    face_v: vec4<f32>,  // 脸的“上”方向（旋转后的 y 轴）
    face_f: vec4<f32>,  // 脸的朝向（旋转后的 z 轴）
};

@group(0) @binding(0) var<uniform> g: Globals;

// ---- 调色板：只有黑、白和几种固定灰 ----
// 表面用非 sRGB 格式，所以数值就是显示字节值 / 255（0.55 ≈ 140）。
const BLACK = vec3<f32>(0.0, 0.0, 0.0);
const WHITE = vec3<f32>(1.0, 1.0, 1.0);
const GRAY_SHADOW = vec3<f32>(0.78, 0.78, 0.78);  // 球的暗面
const GRAY_BG = vec3<f32>(0.55, 0.55, 0.55);      // 背景

// 光源方向（视图空间，指向光源）：左上前方。固定在视图空间，所以球转动时明暗不动，只有脸在转。
const LIGHT_DIR = vec3<f32>(-0.55, 0.65, 0.52);
// dot(n, L) 大于它为亮面，否则为暗面。
const LIGHT_CUT = 0.2;
// 五官只画在 P·F > FACE_MIN 的区域，避免贴到球的侧面被拉得太长。
const FACE_MIN = 0.2;

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    // 一个盖住整个屏幕的大三角形：顶点 (-1,-1)、(3,-1)、(-1,3)
    let x = f32((i << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(i & 2u) * 2.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    return vec4<f32>(shade(frag.xy), 1.0);
}

fn shade(pixel: vec2<f32>) -> vec3<f32> {
    let size = g.screen.xy;
    let radius = g.screen.z;
    let outline = g.screen.w;

    // 像素坐标（原点左上、y 向下）→ 以球心为原点、y 向上的像素偏移（球心 = 窗口中心 + 动作位移）
    let offset = vec2<f32>(pixel.x - size.x * 0.5, size.y * 0.5 - pixel.y) - g.ball.xy;
    // 正交投影：光线沿 -Z 射入，光线到球心的距离就是偏移的长度
    let d = length(offset);
    if d >= radius + outline {
        return GRAY_BG;
    }
    if d >= radius {
        return BLACK;
    }

    // 光线与单位球的交点 P（视图空间，+Z 指向观众）。单位球上法线 n = P。
    let q = offset / radius;
    let p = vec3<f32>(q, sqrt(max(0.0, 1.0 - dot(q, q))));
    if face_ink(p) {
        return BLACK;
    }
    if dot(p, normalize(LIGHT_DIR)) > LIGHT_CUT {
        return WHITE;
    }
    return GRAY_SHADOW;
}

// 脸：把 P 投到脸所朝的平面上，得到脸空间坐标 (u, v) = (P·U, P·V)，五官是这个平面里的 2D SDF。
fn face_ink(p: vec3<f32>) -> bool {
    if dot(p, g.face_f.xyz) <= FACE_MIN {
        return false;
    }
    let uv = vec2<f32>(dot(p, g.face_u.xyz), dot(p, g.face_v.xyz));
    // 第 1 步只有两只圆点眼，用来肉眼确认跟随和旋转；第 3 步换成完整的五官。
    let eye_l = length(uv - vec2<f32>(-0.28, 0.12)) - 0.075;
    let eye_r = length(uv - vec2<f32>(0.28, 0.12)) - 0.075;
    return min(eye_l, eye_r) < 0.0;
}
