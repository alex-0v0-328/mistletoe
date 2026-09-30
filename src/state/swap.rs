//! 换表情的动画节奏：整张脸在竖直方向压扁到 0，在中点换参数，再弹开（略微回弹），同时球轻微放大一下。
//! 只管时间和曲线，不知道表情内容：什么时候该把显示的表情换成目标，由它告诉 State。

use std::f32::consts::PI;

/// 压扁阶段占总时长的比例：前 40% 压扁到 0，之后 60% 弹开
const DOWN: f32 = 0.4;
/// 弹开时的回弹强度（easeOutBack 的常数，约回弹 10%）
const OVERSHOOT: f32 = 1.70158;
/// 换表情时球放大的幅度（在中点最大）
const BALL_POP: f32 = 0.06;

/// 一帧的结果
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SwapFrame {
    /// 脸的竖直缩放：1 = 正常，0 = 压扁到看不见
    pub squish: f32,
    /// 球的缩放
    pub scale: f32,
    /// 这一帧刚好过了中点：调用方应该把显示的表情换成目标
    pub swap_now: bool,
    /// 这一帧动画结束了
    pub finished: bool,
}

const IDLE: SwapFrame = SwapFrame {
    squish: 1.0,
    scale: 1.0,
    swap_now: false,
    finished: false,
};

#[derive(Clone, Copy, Debug)]
struct Running {
    start: f64,
    duration: f64,
    swapped: bool,
}

#[derive(Default)]
pub struct Swap {
    running: Option<Running>,
}

impl Swap {
    /// 开始一次换表情动画。已经在跑就不重来，返回 false。
    pub fn start(&mut self, duration: f64, now: f64) -> bool {
        if self.running.is_some() {
            return false;
        }
        self.running = Some(Running {
            start: now,
            duration,
            swapped: false,
        });
        true
    }

    /// 正在跑、而且还没到中点：这时改目标，中点会直接换成最新的，不用再来一次
    pub fn before_midpoint(&self) -> bool {
        self.running.is_some_and(|r| !r.swapped)
    }

    pub fn active(&self) -> bool {
        self.running.is_some()
    }

    pub fn tick(&mut self, now: f64) -> SwapFrame {
        let Some(mut r) = self.running else {
            return IDLE;
        };
        let p = ((now - r.start) / r.duration).max(0.0) as f32;
        let swap_now = p >= DOWN && !r.swapped;
        r.swapped |= swap_now;
        if p >= 1.0 {
            self.running = None;
            return SwapFrame {
                swap_now,
                finished: true,
                ..IDLE
            };
        }
        self.running = Some(r);
        let squish = if p < DOWN {
            // 压扁：先慢后快
            let x = p / DOWN;
            1.0 - x * x
        } else {
            ease_out_back((p - DOWN) / (1.0 - DOWN))
        };
        SwapFrame {
            squish,
            scale: 1.0 + BALL_POP * (PI * p).sin(),
            swap_now,
            finished: false,
        }
    }
}

/// 0..1 → 0..1，末段先冲过 1 再回来，像弹开
fn ease_out_back(x: f32) -> f32 {
    let c3 = OVERSHOOT + 1.0;
    let t = x - 1.0;
    1.0 + c3 * t * t * t + OVERSHOOT * t * t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn squish_goes_to_zero_swaps_once_then_pops_back() {
        let mut s = Swap::default();
        assert!(s.start(1.0, 0.0));
        assert!(!s.start(1.0, 0.1), "已经在跑就不重来");
        let f = s.tick(0.2);
        assert!(f.squish < 1.0 && f.squish > 0.0 && !f.swap_now);
        let f = s.tick(0.4);
        assert!(f.swap_now && f.squish.abs() < 1e-5, "{f:?}");
        assert!(!s.tick(0.41).swap_now, "中点只换一次");
        let peak = (40..100)
            .map(|i| s.tick(i as f64 / 100.0).squish)
            .fold(0.0, f32::max);
        assert!(peak > 1.02, "弹开时应该略微回弹：{peak}");
        let f = s.tick(1.0);
        assert!(f.finished && f.squish == 1.0 && f.scale == 1.0);
        assert!(!s.active());
    }

    #[test]
    fn ball_scale_peaks_in_the_middle() {
        let mut s = Swap::default();
        s.start(1.0, 0.0);
        let mid = s.tick(0.5).scale;
        assert!((mid - (1.0 + BALL_POP)).abs() < 1e-5);
        assert!(s.tick(0.1).scale < mid);
    }

    #[test]
    fn late_frame_still_reports_the_swap() {
        // 卡了一下，第一帧就已经超过终点：也要告诉调用方换表情
        let mut s = Swap::default();
        s.start(0.3, 0.0);
        let f = s.tick(5.0);
        assert!(f.swap_now && f.finished);
    }
}
