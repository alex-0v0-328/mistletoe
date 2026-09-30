//! 鼠标跟随：把归一化指针位置映射成目标 yaw/pitch，再做与帧率无关的指数平滑。
//! 不知道窗口、像素和时钟；只接收归一化指针和调用方给的时间。

use super::{Pointer, Pose};

/// 指针在窗口边缘时的最大转角：30°。
const MAX_ANGLE: f32 = 30.0_f32.to_radians();
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
    pub fn set_pointer(&mut self, pointer: Option<Pointer>) -> bool {
        let target = match pointer {
            Some(p) => Pose {
                yaw: p.x.clamp(-1.0, 1.0) * MAX_ANGLE,
                pitch: p.y.clamp(-1.0, 1.0) * MAX_ANGLE,
            },
            // 指针离开窗口：回正
            None => Pose::default(),
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

    fn pointer(x: f32, y: f32) -> Option<Pointer> {
        Some(Pointer { x, y })
    }

    #[test]
    fn edge_maps_to_max_angle_and_clamps_beyond() {
        let mut f = Follow::default();
        assert!(f.set_pointer(pointer(1.0, -1.0)));
        assert_eq!(f.target.yaw, MAX_ANGLE);
        assert_eq!(f.target.pitch, -MAX_ANGLE);
        // 超出窗口边缘（拖拽时可能发生）也不超过最大角
        assert!(!f.set_pointer(pointer(3.0, -5.0)));
    }

    #[test]
    fn center_and_leave_mean_no_change_at_rest() {
        let mut f = Follow::default();
        assert!(!f.set_pointer(pointer(0.0, 0.0)));
        assert!(!f.set_pointer(None));
        assert!(f.settled());
    }

    #[test]
    fn first_tick_after_rest_does_not_jump() {
        let mut f = Follow::default();
        f.set_pointer(pointer(1.0, 0.0));
        // 空闲了 100 秒才来第一帧：不能一步到位
        assert_eq!(f.tick(100.0).yaw, 0.0);
        let yaw = f.tick(100.0 + TAU).yaw;
        let expected = MAX_ANGLE * (1.0 - (-1.0f32).exp());
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
    fn settles_exactly_then_returns_on_leave() {
        let mut f = Follow::default();
        f.set_pointer(pointer(-0.5, 0.5));
        let mut t = 0.0;
        while !f.settled() {
            f.tick(t);
            t += 1.0 / 60.0;
            assert!(t < 3.0, "跟随没有在 3 秒内停下");
        }
        assert_eq!(f.current, f.target);
        assert_eq!(f.last_tick, None);
        // 指针离开窗口：目标回到 0，再次开始动画
        assert!(f.set_pointer(None));
        assert!(!f.settled());
    }
}
