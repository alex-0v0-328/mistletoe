//! 鼠标跟随：把指针相对窗口中心的位置映射成目标 yaw/pitch，再做与帧率无关的指数平滑。
//! 不知道窗口、像素和时钟；只接收归一化指针和调用方给的时间。

use super::{Pointer, Pose};

/// 最大转角：30°。指针无限远时才会达到。
const MAX_ANGLE: f32 = 30.0_f32.to_radians();
/// 指针距离（单位：半个窗口短边）等于 KNEE 时，转角约为最大值的 71%（1/√2）。
/// 调小：窗口里一点点移动就转得很明显；调大：要离得更远才转到位。
const KNEE: f32 = 1.0;
/// 平滑时间常数（秒）：x += (target - x) * (1 - exp(-dt / TAU))。
/// 用 exp 而不是固定系数，帧率高低都走同一条曲线。
const TAU: f64 = 0.12;
/// 误差低于这个值（弧度）就吸附到目标并停止动画，否则指数曲线永远到不了终点。
const SNAP: f32 = 0.001;

#[derive(Default)]
pub struct Follow {
    target: Pose,
    current: Pose,
    /// 上一次 tick 的时间。静止时为 None：空闲睡了很久之后，第一帧 dt 按 0 算，不会一步跳到目标。
    last_tick: Option<f64>,
}

impl Follow {
    /// 返回 true 表示目标变了。
    pub fn set_pointer(&mut self, p: Pointer) -> bool {
        // (yaw, pitch) = MAX_ANGLE · (x, y) / √(x² + y² + KNEE²)：
        // 方向始终指向指针；距离越远越接近 MAX_ANGLE，但永远不超过，所以指针在窗口外很远也能平滑地跟随。
        let scale = MAX_ANGLE / (p.x * p.x + p.y * p.y + KNEE * KNEE).sqrt();
        let target = Pose {
            yaw: p.x * scale,
            pitch: p.y * scale,
        };
        let changed = target != self.target;
        self.target = target;
        changed
    }

    pub fn tick(&mut self, now: f64) -> Pose {
        let dt = self.last_tick.map_or(0.0, |last| (now - last).max(0.0));
        let k = (1.0 - (-dt / TAU).exp()) as f32;
        self.current.yaw = approach(self.current.yaw, self.target.yaw, k);
        self.current.pitch = approach(self.current.pitch, self.target.pitch, k);
        self.last_tick = if self.settled() { None } else { Some(now) };
        self.current
    }

    pub fn settled(&self) -> bool {
        self.current == self.target
    }
}

/// 向目标走 k 比例的距离；足够近时直接吸附。
fn approach(x: f32, target: f32, k: f32) -> f32 {
    let next = x + (target - x) * k;
    if (target - next).abs() < SNAP {
        target
    } else {
        next
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pointer(x: f32, y: f32) -> Pointer {
        Pointer { x, y }
    }

    fn angle(pose: Pose) -> f32 {
        (pose.yaw * pose.yaw + pose.pitch * pose.pitch).sqrt()
    }

    #[test]
    fn angle_grows_with_distance_but_never_exceeds_max() {
        let mut f = Follow::default();
        let mut previous = 0.0;
        // 从窗口里一路移到很远的窗口外
        for distance in [0.2, 0.5, 1.0, 3.0, 10.0, 1000.0] {
            f.set_pointer(pointer(distance, 0.0));
            let a = angle(f.target);
            assert!(a > previous && a < MAX_ANGLE, "距离 {distance}：{a}");
            previous = a;
        }
        assert!(previous > MAX_ANGLE * 0.999);
        f.set_pointer(pointer(KNEE, 0.0));
        assert!((angle(f.target) - MAX_ANGLE / 2.0f32.sqrt()).abs() < 1e-6);
    }

    #[test]
    fn target_points_toward_pointer_in_every_quadrant() {
        let mut f = Follow::default();
        for (x, y) in [(3.0, 1.0), (-2.0, 5.0), (-4.0, -4.0), (0.5, -8.0)] {
            f.set_pointer(pointer(x, y));
            // 同号，且 yaw:pitch 等于 x:y
            assert!(f.target.yaw * x > 0.0 && f.target.pitch * y > 0.0);
            assert!((f.target.yaw * y - f.target.pitch * x).abs() < 1e-6);
        }
    }

    #[test]
    fn center_means_no_change_at_rest() {
        let mut f = Follow::default();
        assert!(!f.set_pointer(pointer(0.0, 0.0)));
        assert!(f.settled());
    }

    #[test]
    fn first_tick_after_rest_does_not_jump() {
        let mut f = Follow::default();
        f.set_pointer(pointer(1.0, 0.0));
        let target = f.target.yaw;
        // 空闲了 100 秒才来第一帧：不能一步到位
        assert_eq!(f.tick(100.0).yaw, 0.0);
        let yaw = f.tick(100.0 + TAU).yaw;
        let expected = target * (1.0 - (-1.0f32).exp());
        assert!((yaw - expected).abs() < 1e-5, "{yaw} vs {expected}");
    }

    #[test]
    fn smoothing_is_frame_rate_independent() {
        let mut coarse = Follow::default();
        let mut fine = Follow::default();
        coarse.set_pointer(pointer(0.6, 0.4));
        fine.set_pointer(pointer(0.6, 0.4));
        coarse.tick(0.0);
        fine.tick(0.0);
        coarse.tick(0.1);
        for i in 1..=10 {
            fine.tick(i as f64 * 0.01);
        }
        assert!((coarse.current.yaw - fine.current.yaw).abs() < 1e-5);
        assert!((coarse.current.pitch - fine.current.pitch).abs() < 1e-5);
    }

    #[test]
    fn settles_exactly_then_restarts_on_move() {
        let mut f = Follow::default();
        f.set_pointer(pointer(-5.0, 2.0));
        let mut t = 0.0;
        while !f.settled() {
            f.tick(t);
            t += 1.0 / 60.0;
            assert!(t < 3.0, "跟随没有在 3 秒内停下");
        }
        assert_eq!(f.current, f.target);
        assert_eq!(f.last_tick, None);
        // 指针再动一下：重新开始动画
        assert!(f.set_pointer(pointer(-5.0, 3.0)));
        assert!(!f.settled());
    }
}
