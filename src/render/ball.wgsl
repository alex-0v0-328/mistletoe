// 球的着色器（按网页原型 prototype_reference.frag 的做法移植，颜色只用黑白灰）：
// 全屏三角形 + 逐像素求视线与球的关系，画出轮廓、两阶硬明暗、受光侧的白色边缘光，以及一张 2D SDF 拼成的脸。
// 所有部件都用 fwidth 抗锯齿；区域内部是纯色，不做渐变和柔和阴影。不要在这里加随时间变化的效果：空闲时不重绘。

// 与 render/mod.rs 里的 Globals 逐字节对应；只用 vec4<f32>，避开 WGSL 对齐坑。
struct Globals {
    screen: vec4<f32>,   // x = 宽，y = 高（物理像素），z = 球半径（像素），w = 外轮廓宽度（球半径的倍数）
    ball: vec4<f32>,     // xy = 球心相对窗口中心的位移（像素，y 向上），zw 未用
    face_u: vec4<f32>,   // 球的本地 x 轴（视图空间）= 脸的“右”
    face_v: vec4<f32>,   // 球的本地 y 轴 = 脸的“上”
    face_f: vec4<f32>,   // 球的本地 z 轴 = 脸的朝向
    expr: vec4<f32>,     // x = 眼型编号，y = 睁眼程度 1..0，z = 嘴型编号，w = 脸的竖直缩放（换表情时压扁到 0 再弹开）
    mouth: vec4<f32>,    // x = 嘴宽 0..1，y = 张开 0..1，z = 弯曲 -1..1，w 未用
    overlay: vec4<f32>,  // x = 腮红，y = 眼泪，z = 汗，w = 阴沉脸，都是 0..1 的显现程度
    extra: vec4<f32>,    // x = 鼻涕泡的显现程度，yz = 眼神偏移（脸空间），w 未用
};

@group(0) @binding(0) var<uniform> g: Globals;

// ---- 调色板：只有黑白灰。表面用非 sRGB 格式，数值就是显示字节值 / 255 ----
const BACKGROUND = vec3<f32>(0.55, 0.55, 0.55);  // 窗口背景，不属于画面
const LIT = vec3<f32>(0.92, 0.92, 0.92);         // 亮面：比纯白暗一点，白色边缘光才看得见
const SHADE = vec3<f32>(0.72, 0.72, 0.72);       // 暗面；舌头
const WHITE = vec3<f32>(1.0, 1.0, 1.0);          // 边缘光；牙齿、泪、汗、鼻涕泡的填充
const BLACK = vec3<f32>(0.0, 0.0, 0.0);          // 轮廓和所有线条

const PI = 3.14159265;
const FAR = 1e5;             // 某个部件不画时返回的距离：足够远，覆盖率为 0
// 光源方向（视图空间，指向光源）：左上、偏向观众，所以暗面只是背光的一小块
const LIGHT_DIR = vec3<f32>(-0.45, 0.5, 0.8);
const SHADE_EDGE = 0.012;    // N·L 的过渡半宽：很窄，切成两阶硬边

// ---- 脸空间：q = 本地 xy / FACE_SCALE，下面的尺寸都在 q 里（和原型的参数一致）----
const FACE_SCALE = 1.55;     // 放大五官：两眼外侧相距约为球直径的一半
const LW = 0.018;            // 线的半宽
const MIN_LW_PX = 1.5;       // 球很小时线的半宽至少这么多像素
const EYE_SPACING = 0.26;    // 眼睛中心离中线的距离
const EYE_Y = 0.0;
const EYE_SIZE = 0.1;
const MOUTH_Y = -0.17;
const BUBBLE_AT = vec2<f32>(0.11, -0.12);

// ---- 阴沉脸的竖线（在球的本地空间里量，单位是球半径）----
const GLOOM_STEP = 0.11;     // 相邻两道长线的间距（本地 x）；短线插在正中间
const GLOOM_SPAN = 0.6;      // 竖线只画在本地 x 的 ±GLOOM_SPAN 以内，也就是脸的范围
const GLOOM_TOP = 0.9;       // 竖线上端的最低高度（本地 y）：高到球的轮廓附近，线像从脸的顶端垂下来
const GLOOM_HALO = 0.03;     // 眼睛、眉毛、嘴周围留出的空白（脸空间），竖线在这里断开

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

// 注意：这里和 draw_face 里都不能有按像素分支的提前 return —— WGSL 只允许在统一控制流里调用 fwidth。
fn shade(pixel: vec2<f32>) -> vec3<f32> {
    let radius = g.screen.z;
    // 像素坐标（原点左上、y 向下）→ 以球心为原点、y 向上的像素偏移
    let offset = vec2<f32>(pixel.x - g.screen.x * 0.5, g.screen.y * 0.5 - pixel.y) - g.ball.xy;
    // 正交投影：视线沿 -Z，视线到球心的最近距离就是 offset 的长度
    let dist = length(offset);
    let s = offset / radius;
    // 球面法线（视图空间，+Z 指向观众）；球外的像素算出来的值会被轮廓盖掉
    let n = vec3<f32>(s, sqrt(max(0.0, 1.0 - dot(s, s))));
    // 转到球的本地空间：p = Rᵀ·n。p.xy 直接当脸部坐标（正交投影），球转动时五官自然透视变形
    let p = vec3<f32>(dot(n, g.face_u.xyz), dot(n, g.face_v.xyz), dot(n, g.face_f.xyz));

    // ---- 明暗：N·L 用很窄的 smoothstep 切成两阶 ----
    let l = dot(n, normalize(LIGHT_DIR));
    var col = mix(SHADE, LIT, smoothstep(-SHADE_EDGE, SHADE_EDGE, l));
    // ---- 边缘光：菲涅尔值用 smoothstep 切硬，得到轮廓内侧一圈细白边，只出现在受光一侧 ----
    let fres = 1.0 - n.z;
    let rim = smoothstep(0.62, 0.66, fres) * smoothstep(-0.05, 0.15, l);
    col = mix(col, WHITE, rim);

    col = draw_face(col, p);

    // ---- 轮廓：视线到球心的距离和半径比较，fwidth 抗锯齿 ----
    let pw = fwidth(dist) + 1e-5;
    let sphere = 1.0 - smoothstep(radius - pw, radius + pw, dist);
    let r_out = radius * (1.0 + g.screen.w);
    let outline = 1.0 - smoothstep(r_out - pw, r_out + pw, dist);
    return mix(mix(BACKGROUND, BLACK, outline), col, sphere);
}

// 距离 d（< 0 在里面）→ 覆盖率，边缘按 fwidth 过渡
fn aaf(d: f32) -> f32 {
    let w = fwidth(d) * 0.8 + 1e-5;
    return 1.0 - smoothstep(-w, w, d);
}

// 五官和叠加层：颜色不受光照影响，始终是纯色
fn draw_face(base: vec3<f32>, p: vec3<f32>) -> vec3<f32> {
    let squish = g.expr.w;
    var q = p.xy / FACE_SCALE;
    // 换表情：整张脸在 y 方向压扁
    q.y /= max(squish, 0.02);
    // 脸的边缘用本地 z 柔和裁切：五官转到侧面时自然淡出；压得太扁时整张脸隐藏
    let fclip = smoothstep(0.02, 0.22, p.z) * step(0.04, squish);
    let lw = max(LW, MIN_LW_PX / (g.screen.z * FACE_SCALE));
    let look = g.extra.yz;

    // 眼睛：镜像坐标，x 指向鼻子一侧，一个函数画两只眼
    let e = vec2<f32>(EYE_SPACING - abs(q.x - look.x), q.y - EYE_Y - look.y);
    // 嘴跟着眼神偏移得少一些
    let m = vec2<f32>(q.x - look.x * 0.6, q.y - MOUTH_Y - look.y * 0.6);
    let eye = eye_sd(e, lw);
    let mouth = mouth_sd(m, lw);
    let gloom = gloom_sd(p, lw * FACE_SCALE * 0.55);
    let tear = tears_sd(q);
    let sweat = sweat_sd(q);
    let bubble = bubble_sd(q);

    var col = base;
    // 阴沉脸的竖线在最底层，眼睛、眉毛和嘴周围留一圈空白，保证五官清楚
    let clear = (1.0 - aaf(eye - GLOOM_HALO)) * (1.0 - aaf(mouth.x - GLOOM_HALO));
    col = mix(col, BLACK, aaf(gloom) * clear * fclip);
    col = mix(col, BLACK, aaf(blush_sd(q)) * fclip);
    col = mix(col, WHITE, aaf(tear) * fclip);
    col = mix(col, BLACK, aaf(abs(tear) - lw * 0.7) * fclip);
    col = mix(col, WHITE, aaf(mouth.z) * fclip);
    col = mix(col, BLACK, max(aaf(eye), aaf(mouth.x)) * fclip);
    col = mix(col, SHADE, aaf(mouth.y) * fclip);
    col = mix(col, WHITE, aaf(bubble) * fclip);
    col = mix(col, BLACK, aaf(abs(bubble) - lw * 0.6) * fclip);
    col = mix(col, WHITE, aaf(sweat) * fclip);
    col = mix(col, BLACK, aaf(abs(sweat) - lw * 0.7) * fclip);
    return col;
}

// ================= 眼睛 =================
// e：眼睛局部坐标，+x 指向鼻子。眨眼只压扁 y：实心眼型压 y 坐标（最扁到 0.22，闭上时仍是一条线），
// 弧线类压扁拱高，线宽不变。编号与 render/mod.rs 的 eye_code 对应。

fn eye_sd(e: vec2<f32>, lw: f32) -> f32 {
    let shape = u32(g.expr.x + 0.5);
    let open = g.expr.y;
    let s = EYE_SIZE;
    let ey = vec2<f32>(e.x, e.y / max(open, 0.22));
    switch shape {
        // smile：^
        case 1u: {
            return curve_sd(e, s, -0.9 * s * open, 0.0) - lw;
        }
        // closed：往下弯的 ∪，安心或困倦地闭着眼
        case 2u: {
            return curve_sd(e, s, 0.7 * s * open, 0.0) - lw;
        }
        // squint：眯成略微拱起的一笔粗线
        case 3u: {
            return curve_sd(e, s, -0.35 * s * open, 0.0) - lw * 1.25;
        }
        // wide：圆圈 + 小瞳孔。眨眼一开始白眼珠就被填成黑色，再整体压扁，不会变成空心的扁框
        case 4u: {
            let w = vec2<f32>(e.x, e.y / max(open, 0.05));
            let r = length(w);
            let ring = min(abs(r - 0.8 * s) - lw, r - 0.3 * s);
            let solid = r - 0.8 * s - lw;
            return mix(ring, solid, 1.0 - smoothstep(0.6, 0.95, open));
        }
        // sad：稍小的实心眼 + 明显的眉毛（靠鼻子一侧高、外侧低）
        case 5u: {
            let brow = sd_seg(e, vec2<f32>(0.5 * s, 1.55 * s), vec2<f32>(-0.95 * s, 1.05 * s)) - lw * 1.1;
            return min(sd_ellipse(ey, vec2<f32>(0.55, 0.72) * s), brow);
        }
        // annoyed：半睁眼，平直的上眼皮压住眼睛上半部；眉毛靠鼻子一侧低、外侧高（往中间下斜）
        case 6u: {
            let lid_y = 0.1 * s * max(open, 0.22);
            let body = max(sd_ellipse(ey, vec2<f32>(0.62, 0.72) * s), e.y - lid_y);
            let lid = sd_seg(e, vec2<f32>(-0.95 * s, lid_y), vec2<f32>(0.9 * s, lid_y)) - lw * 1.2;
            let brow = sd_seg(e, vec2<f32>(0.6 * s, 1.05 * s), vec2<f32>(-0.95 * s, 1.5 * s)) - lw * 1.1;
            return min(min(body, lid), brow);
        }
        // dot：竖着的实心椭圆
        default: {
            return sd_ellipse(ey, vec2<f32>(0.62, 0.8) * s);
        }
    }
}

// ================= 嘴 =================
// m：嘴的局部坐标。返回 x = 黑色（线、口腔、牙缝）的 SDF，y = 舌头的 SDF，z = 牙齿（白色）的 SDF。
// 编号与 render/mod.rs 的 mouth_code 对应。

fn mouth_sd(m: vec2<f32>, lw: f32) -> vec3<f32> {
    let shape = u32(g.expr.z + 0.5);
    let w = mix(0.04, 0.16, g.mouth.x);         // 半宽
    let h = g.mouth.y * (0.4 * w + 0.05);       // 张开的高度
    let k = g.mouth.z * (0.45 * w + 0.01);      // 弯曲：k > 0 嘴角上扬
    switch shape {
        // cat：ω，两段小 ∪ 并排
        case 1u: {
            let c = vec2<f32>(abs(m.x) - w * 0.5, m.y);
            return vec3<f32>(curve_sd(c, w * 0.5, max(k, 0.02) * 0.8, 0.0) - lw, FAR, FAR);
        }
        // triangle：倒三角的大张嘴，底部一块舌头
        case 2u: {
            let hh = max(h, 0.03);
            let tri = sd_tri(m, vec2<f32>(-w, hh * 0.35), vec2<f32>(w, hh * 0.35), vec2<f32>(0.0, -hh * 0.9)) - lw * 0.4;
            let tongue = max(tri + lw * 1.4, length(m - vec2<f32>(0.0, -hh * 0.75)) - w * 0.4);
            return vec3<f32>(tri, tongue, FAR);
        }
        // wavy：圆滑的波浪线，一个半波
        case 3u: {
            return vec3<f32>(curve_sd(m, w, k, 0.3 * w) - lw, FAR, FAR);
        }
        // grin：露齿笑。半椭圆和一条直线求交得到 D 形，描边取绝对值；牙齿用 fract 画竖线并裁在嘴里
        case 4u: {
            let hh = max(h, 0.03);
            let mm = vec2<f32>(m.x, m.y - k * 0.6 * (m.x * m.x) / (w * w));
            let d_shape = max(mm.y, sd_ellipse(mm, vec2<f32>(w, hh)));
            let sp = 2.0 * w / 5.0;
            let gap = abs(fract((mm.x + w) / sp + 0.5) - 0.5) * sp - lw * 0.8;
            return vec3<f32>(min(abs(d_shape) - lw, max(gap, d_shape)), FAR, d_shape);
        }
        // line：一笔弧线；张开时，沿弧线挖一个黑色开口（窄而不弯时是 O，笑着是 D），底部一块舌头
        default: {
            let line = curve_sd(m, w, k, 0.0) - lw;
            if h < 0.004 {
                return vec3<f32>(line, FAR, FAR);
            }
            let t = clamp(m.x / w, -1.0, 1.0);
            let mm = vec2<f32>(m.x, m.y - k * (t * t - 0.5));
            let hole = sd_ellipse(mm - vec2<f32>(0.0, -0.3 * h), vec2<f32>(w, 0.65 * h));
            var tongue = FAR;
            if h > 0.02 {
                tongue = max(hole + lw * 1.2, length(mm - vec2<f32>(0.0, -0.85 * h)) - w * 0.5);
            }
            return vec3<f32>(min(line, hole), tongue, FAR);
        }
    }
}

// ================= 叠加层 =================

// 腮红：用 fract 画斜条纹，裁在两颊的椭圆里；椭圆随 blush 从中心长大（显露，不是淡入）
fn blush_sd(q: vec2<f32>) -> f32 {
    let amount = g.overlay.x;
    if amount <= 0.0 {
        return FAR;
    }
    let bq = vec2<f32>(abs(q.x) - (EYE_SPACING + 0.1), q.y - (EYE_Y - 0.15));
    let area = sd_ellipse(bq, vec2<f32>(0.085, 0.04) * amount);
    // 条纹方向左右镜像：两边都向外倾斜
    let v = (abs(q.x) - q.y * 0.55) / 0.034;
    let stripes = (abs(fract(v) - 0.5) - 0.2) * 0.034;
    return max(area, stripes);
}

// 眼泪：眼睛下方往下流的圆角矩形泪痕，带一点正弦摆动（固定形状，不随时间动）；和眼睛之间留空隙，
// 随 tears 往下延伸
fn tears_sd(q: vec2<f32>) -> f32 {
    let amount = g.overlay.y;
    if amount <= 0.0 {
        return FAR;
    }
    var t = vec2<f32>(abs(q.x) - EYE_SPACING, q.y - (EYE_Y - 0.1));
    t.x += 0.008 * sin(t.y * 32.0);
    let half_len = 0.22 * amount;
    let b = abs(t - vec2<f32>(0.0, -0.02 - half_len)) - vec2<f32>(0.026, half_len);
    return length(max(b, vec2<f32>(0.0))) + min(max(b.x, b.y), 0.0) - 0.012;
}

// 汗滴：右额角偏外，两个圆用 smooth min 融合成水滴；随 sweat 从上方滑下来并变大
fn sweat_sd(q: vec2<f32>) -> f32 {
    let amount = g.overlay.z;
    if amount <= 0.0 {
        return FAR;
    }
    // 原型的位置是 (EYE_SPACING + 0.28, 0.2)；五官放大后那里贴到了轮廓上，所以往里收一些、再放大
    let scale = 0.065 * mix(0.4, 1.0, amount);
    let center = vec2<f32>(EYE_SPACING + 0.17, EYE_Y + 0.2 + 0.1 * (1.0 - amount));
    let sq = (q - center) / scale;
    // 原型是 (0, 1.9)、半径 0.12、k = 0.9；加了黑色描边后细脖子像一根茎，所以尖端的小圆大一点、融合更宽
    return smin(length(sq) - 1.0, length(sq - vec2<f32>(0.0, 1.75)) - 0.2, 1.1) * scale;
}

// 鼻涕泡：困的时候两眼之间偏下的一个小泡泡，随 bubble 从无长到最大
fn bubble_sd(q: vec2<f32>) -> f32 {
    let amount = g.extra.x;
    if amount <= 0.0 {
        return FAR;
    }
    return length(q - BUBBLE_AT) - 0.048 * amount;
}

// 阴沉脸：不画任何灰色块，只用黑色竖线表现阴影。
// 每道竖线是球本地空间里 x = 常数 的截线：画在球面上，正面看是平行的竖线，球转动时跟着球面弯曲。
// 长线从脸的上沿垂到眼睛附近，短线插在长线中间、只画在上半部，所以上密下疏；
// 每道线的上端、下端都错开，下端收尖，不形成整齐的边界。返回到最近一道线的距离（本地空间单位）。
fn gloom_sd(p: vec3<f32>, lw: f32) -> f32 {
    let amount = g.overlay.w;
    if amount <= 0.0 {
        return FAR;
    }
    let y = p.y / max(g.expr.w, 0.02);  // 换表情时跟着整张脸一起压扁
    var d = FAR;
    for (var layer = 0; layer < 2; layer++) {
        let shift = 0.5 * f32(layer);
        // 只看左右最近的两道线，距离在格子交界处连续，抗锯齿才不会出现接缝
        let k0 = floor(p.x / GLOOM_STEP - shift);
        for (var j = 0; j < 2; j++) {
            let k = k0 + f32(j);
            let x = (k + shift) * GLOOM_STEP;
            d = min(d, gloom_line(p.x, y, x, k + 37.0 * f32(layer), layer == 1, amount, lw));
        }
    }
    return d;
}

// 一道竖线：位于本地 x = line_x，从上端画到下端；线宽先慢慢变粗，再收尖到下端
fn gloom_line(px: f32, y: f32, line_x: f32, id: f32, short: bool, amount: f32, lw: f32) -> f32 {
    let top = GLOOM_TOP + 0.1 * hash(id + 7.0);
    var bottom = 0.02 + 0.2 * hash(id);
    if short {
        bottom = 0.32 + 0.18 * hash(id);
    }
    // 越靠两侧收得越早
    bottom = max(bottom, 0.4 * smoothstep(0.35, GLOOM_SPAN, abs(line_x)));
    // 经过眼睛上方的线停在眼睛上面
    let eye_x = EYE_SPACING * FACE_SCALE;
    if abs(abs(line_x) - eye_x) < 0.62 * EYE_SIZE * FACE_SCALE + 0.04 {
        bottom = max(bottom, (EYE_Y + 0.8 * EYE_SIZE) * FACE_SCALE + 0.05);
    }
    // 脸的范围以外的线长度为 0：不画，但距离仍然连续（直接跳过会在边界上抗锯齿出一道接缝）
    if abs(line_x) > GLOOM_SPAN {
        bottom = top;
    }
    // 随 amount 从上往下长出来
    let end = mix(top, bottom, amount);
    let len = max(top - end, 1e-4);
    let dx = abs(px - line_x);
    let t = (top - y) / len;  // 0 = 上端，1 = 下端
    if t <= 0.0 {
        return length(vec2<f32>(dx, y - top));
    }
    if t >= 1.0 {
        return length(vec2<f32>(dx, y - end));
    }
    // 上端慢慢变粗（顶部不糊成一片），下端收尖
    let taper = min(t / 0.15, 1.0) * pow(1.0 - t, 0.4);
    return dx - lw * taper;
}

// ================= 2D SDF 工具（和原型相同）=================

fn sd_seg(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> f32 {
    let pa = p - a;
    let ba = b - a;
    let h = clamp(dot(pa, ba) / dot(ba, ba), 0.0, 1.0);
    return length(pa - ba * h);
}

// 椭圆的近似距离；正中心单独处理，否则那一点会得到 0（边缘的覆盖率）
fn sd_ellipse(p: vec2<f32>, r: vec2<f32>) -> f32 {
    let k0 = length(p / r);
    let k1 = length(p / (r * r));
    return select(k0 * (k0 - 1.0) / k1, -min(r.x, r.y), k1 < 1e-6);
}

fn smin(a: f32, b: f32, k: f32) -> f32 {
    let h = clamp(0.5 + 0.5 * (b - a) / k, 0.0, 1.0);
    return mix(b, a, h) - k * h * (1.0 - h);
}

fn sd_tri(p: vec2<f32>, p0: vec2<f32>, p1: vec2<f32>, p2: vec2<f32>) -> f32 {
    let e0 = p1 - p0;
    let e1 = p2 - p1;
    let e2 = p0 - p2;
    let v0 = p - p0;
    let v1 = p - p1;
    let v2 = p - p2;
    let pq0 = v0 - e0 * clamp(dot(v0, e0) / dot(e0, e0), 0.0, 1.0);
    let pq1 = v1 - e1 * clamp(dot(v1, e1) / dot(e1, e1), 0.0, 1.0);
    let pq2 = v2 - e2 * clamp(dot(v2, e2) / dot(e2, e2), 0.0, 1.0);
    let s = sign(e0.x * e2.y - e0.y * e2.x);
    let d = min(min(vec2<f32>(dot(pq0, pq0), s * (v0.x * e0.y - v0.y * e0.x)),
                    vec2<f32>(dot(pq1, pq1), s * (v1.x * e1.y - v1.y * e1.x))),
                vec2<f32>(dot(pq2, pq2), s * (v2.x * e2.y - v2.y * e2.x)));
    return -sqrt(d.x) * sign(d.y);
}

// 抛物线折线：y = k·((x/w)² − 0.5) + wave·sin(1.5π·x/w)，x ∈ [−w, w]，用 12 段线段求距离；端点天然是圆头
fn curve_sd(p: vec2<f32>, w: f32, k: f32, wave: f32) -> f32 {
    var d = FAR;
    var prev = vec2<f32>(0.0);
    for (var i = 0; i <= 12; i++) {
        let t = -1.0 + 2.0 * f32(i) / 12.0;
        let c = vec2<f32>(t * w, k * (t * t - 0.5) + wave * sin(1.5 * PI * t));
        if i > 0 {
            d = min(d, sd_seg(p, prev, c));
        }
        prev = c;
    }
    return d;
}

// 由整数编号得到固定的 0..1 伪随机数（只用来让阴沉脸的竖线长短不一）
fn hash(k: f32) -> f32 {
    return fract(sin(k * 12.9898 + 1.0) * 43758.5453);
}
