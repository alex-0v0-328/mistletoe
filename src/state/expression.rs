//! 表情：眼型、嘴型、叠加层的数据结构，“部分表情”的合并与校验，以及显示值的平滑过渡。
//! 不管眨眼计时：眼型在眨眼完全闭上的那一刻切换，这个时刻由 State 从 blink 拿到后告诉这里。

use serde::{Deserialize, Serialize};

use super::{check_range, lerp, smoothstep};

/// 换嘴型的总时长（秒）：前半段压成一条线，中点换形状，后半段展开。
const MOUTH_SWAP: f64 = 0.2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Eyes {
    Dot,
    Smile,
    Closed,
    Squint,
    Wide,
    Sad,
}

impl Eyes {
    pub const ALL: [Eyes; 6] = [
        Eyes::Dot,
        Eyes::Smile,
        Eyes::Closed,
        Eyes::Squint,
        Eyes::Wide,
        Eyes::Sad,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MouthShape {
    Line,
    Cat,
    Triangle,
    Wavy,
}

impl MouthShape {
    pub const ALL: [MouthShape; 4] = [
        MouthShape::Line,
        MouthShape::Cat,
        MouthShape::Triangle,
        MouthShape::Wavy,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Mouth {
    pub shape: MouthShape,
    /// 宽度 0..1
    pub width: f32,
    /// 张开程度 0..1
    pub open: f32,
    /// 弯曲 -1..1，负数是撇嘴
    pub curve: f32,
}

/// 完整表情。叠加层（腮红、眼泪、汗、阴沉脸）都是 0..1 的显现程度。
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Expression {
    pub eyes: Eyes,
    pub mouth: Mouth,
    pub blush: f32,
    pub tears: f32,
    pub sweat: f32,
    pub gloom: f32,
}

/// 部分表情：只写要改的字段。presets.json 和 set_expression 命令都用它。
#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpressionPatch {
    pub eyes: Option<Eyes>,
    pub mouth: Option<MouthPatch>,
    pub blush: Option<f32>,
    pub tears: Option<f32>,
    pub sweat: Option<f32>,
    pub gloom: Option<f32>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MouthPatch {
    pub shape: Option<MouthShape>,
    pub width: Option<f32>,
    pub open: Option<f32>,
    pub curve: Option<f32>,
}

impl ExpressionPatch {
    /// 检查数值范围。越界是错误，不做静默截断：用户调试时看到的就是 agent 会遇到的。
    pub fn validate(&self) -> Result<(), String> {
        let m = self.mouth.unwrap_or_default();
        check_range("mouth.width", m.width, 0.0, 1.0)?;
        check_range("mouth.open", m.open, 0.0, 1.0)?;
        check_range("mouth.curve", m.curve, -1.0, 1.0)?;
        check_range("blush", self.blush, 0.0, 1.0)?;
        check_range("tears", self.tears, 0.0, 1.0)?;
        check_range("sweat", self.sweat, 0.0, 1.0)?;
        check_range("gloom", self.gloom, 0.0, 1.0)
    }

    /// 把给了的字段覆盖到 base 上。
    pub fn apply_to(&self, base: Expression) -> Expression {
        let m = self.mouth.unwrap_or_default();
        Expression {
            eyes: self.eyes.unwrap_or(base.eyes),
            mouth: Mouth {
                shape: m.shape.unwrap_or(base.mouth.shape),
                width: m.width.unwrap_or(base.mouth.width),
                open: m.open.unwrap_or(base.mouth.open),
                curve: m.curve.unwrap_or(base.mouth.curve),
            },
            blush: self.blush.unwrap_or(base.blush),
            tears: self.tears.unwrap_or(base.tears),
            sweat: self.sweat.unwrap_or(base.sweat),
            gloom: self.gloom.unwrap_or(base.gloom),
        }
    }

    /// 所有字段都给了才能当完整表情用（neutral 预设要求这样）。
    pub fn complete(&self) -> Option<Expression> {
        let m = self.mouth?;
        Some(Expression {
            eyes: self.eyes?,
            mouth: Mouth {
                shape: m.shape?,
                width: m.width?,
                open: m.open?,
                curve: m.curve?,
            },
            blush: self.blush?,
            tears: self.tears?,
            sweat: self.sweat?,
            gloom: self.gloom?,
        })
    }
}

/// 这一帧实际显示的脸：连续字段是过渡中的值，眼型和嘴型是此刻显示的形状。
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Face {
    pub eyes: Eyes,
    pub mouth: Mouth,
    /// 换嘴型时压成一条线的程度 0..1（1 = 完全压平）。render 用它把嘴的所有起伏一起压平。
    pub mouth_flat: f32,
    pub blush: f32,
    pub tears: f32,
    pub sweat: f32,
    pub gloom: f32,
    /// 眨眼闭合程度 0（睁开）..1（闭上）
    pub blink: f32,
}

/// 表情的过渡状态。
pub struct ExpressionState {
    target: Expression,
    /// 过渡起点：设新目标那一刻的显示值
    from: Expression,
    start: f64,
    duration: f64,
    shown_eyes: Eyes,
    /// 眼型切换的时刻（眨眼完全闭上的那一刻）；None = 没有待切换的眼型
    eye_swap_at: Option<f64>,
    shown_mouth: MouthShape,
    mouth_swap: Option<MouthSwap>,
}

/// 一次“压平 → 换形状 → 展开”的嘴型切换。
#[derive(Clone, Copy)]
struct MouthSwap {
    start: f64,
    /// 已经在中点换过形状了；展开阶段目标再变，要等这次结束后另起一次
    swapped: bool,
}

impl ExpressionState {
    pub fn new(initial: Expression) -> Self {
        Self {
            target: initial,
            from: initial,
            start: 0.0,
            duration: 0.0,
            shown_eyes: initial.eyes,
            eye_swap_at: None,
            shown_mouth: initial.mouth.shape,
            mouth_swap: None,
        }
    }

    pub fn target(&self) -> Expression {
        self.target
    }

    /// 设新目标，连续字段从当前显示值开始过渡。duration 为 0 表示所有字段立即切换。
    /// 返回 true 表示眼型要换、需要一次眨眼：调用方安排好眨眼后，用 swap_eyes_at 告诉这里闭眼时刻。
    pub fn set_target(&mut self, target: Expression, duration: f64, now: f64) -> bool {
        self.from = self.blend(now);
        self.target = target;
        self.start = now;
        self.duration = duration;
        if duration <= 0.0 {
            self.shown_eyes = target.eyes;
            self.eye_swap_at = None;
            self.shown_mouth = target.mouth.shape;
            self.mouth_swap = None;
            return false;
        }
        if self.mouth_swap.is_none() && self.shown_mouth != target.mouth.shape {
            self.mouth_swap = Some(MouthSwap {
                start: now,
                swapped: false,
            });
        }
        if target.eyes == self.shown_eyes {
            // 又换回了正在显示的眼型：取消待切换
            self.eye_swap_at = None;
            return false;
        }
        // 已经在等一次眨眼了：到时换成最新的目标，不用再眨一次
        self.eye_swap_at.is_none()
    }

    pub fn swap_eyes_at(&mut self, at: f64) {
        self.eye_swap_at = Some(at);
    }

    /// 推进到 now，给出显示的脸（blink 由调用方填）。
    pub fn tick(&mut self, now: f64) -> Face {
        if let Some(at) = self.eye_swap_at
            && now >= at
        {
            self.shown_eyes = self.target.eyes;
            self.eye_swap_at = None;
        }

        let mut mouth_flat = 0.0;
        if let Some(swap) = &mut self.mouth_swap {
            let p = ((now - swap.start) / MOUTH_SWAP) as f32;
            if p >= 0.5 && !swap.swapped {
                // 中点：嘴已经压成一条线，这时换形状看不出跳变
                self.shown_mouth = self.target.mouth.shape;
                swap.swapped = true;
            }
            if p < 1.0 {
                // 三角波 0 → 1 → 0，再用 smoothstep 让两端柔和
                mouth_flat = smoothstep(1.0 - (2.0 * p - 1.0).abs());
            } else if self.shown_mouth != self.target.mouth.shape {
                self.mouth_swap = Some(MouthSwap {
                    start: now,
                    swapped: false,
                });
            } else {
                self.mouth_swap = None;
            }
        }

        let e = self.blend(now);
        Face {
            eyes: self.shown_eyes,
            mouth: Mouth {
                shape: self.shown_mouth,
                ..e.mouth
            },
            mouth_flat,
            blush: e.blush,
            tears: e.tears,
            sweat: e.sweat,
            gloom: e.gloom,
            blink: 0.0,
        }
    }

    pub fn animating(&self, now: f64) -> bool {
        now < self.start + self.duration || self.eye_swap_at.is_some() || self.mouth_swap.is_some()
    }

    /// 连续字段在 now 时刻的过渡值；离散字段直接取目标（显示用的离散字段另算）。
    fn blend(&self, now: f64) -> Expression {
        let t = if self.duration <= 0.0 {
            1.0
        } else {
            smoothstep(((now - self.start) / self.duration) as f32)
        };
        let (a, b) = (self.from, self.target);
        Expression {
            eyes: b.eyes,
            mouth: Mouth {
                shape: b.mouth.shape,
                width: lerp(a.mouth.width, b.mouth.width, t),
                open: lerp(a.mouth.open, b.mouth.open, t),
                curve: lerp(a.mouth.curve, b.mouth.curve, t),
            },
            blush: lerp(a.blush, b.blush, t),
            tears: lerp(a.tears, b.tears, t),
            sweat: lerp(a.sweat, b.sweat, t),
            gloom: lerp(a.gloom, b.gloom, t),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn neutral() -> Expression {
        Expression {
            eyes: Eyes::Dot,
            mouth: Mouth {
                shape: MouthShape::Line,
                width: 0.35,
                open: 0.0,
                curve: 0.2,
            },
            blush: 0.0,
            tears: 0.0,
            sweat: 0.0,
            gloom: 0.0,
        }
    }

    fn with_blush(blush: f32) -> Expression {
        Expression { blush, ..neutral() }
    }

    #[test]
    fn continuous_fields_ease_from_start_to_target() {
        let mut s = ExpressionState::new(neutral());
        s.set_target(with_blush(1.0), 0.2, 1.0);
        assert_eq!(s.tick(1.0).blush, 0.0);
        assert!((s.tick(1.1).blush - 0.5).abs() < 1e-6);
        assert_eq!(s.tick(1.2).blush, 1.0);
        assert!(!s.animating(1.2));
    }

    #[test]
    fn retarget_mid_transition_continues_from_shown_value() {
        let mut s = ExpressionState::new(neutral());
        s.set_target(with_blush(1.0), 0.2, 0.0);
        let shown = s.tick(0.05).blush;
        s.set_target(with_blush(0.0), 0.2, 0.05);
        // 改目标的那一刻不能跳
        assert!((s.tick(0.05).blush - shown).abs() < 1e-6);
        assert_eq!(s.tick(0.25).blush, 0.0);
    }

    #[test]
    fn mouth_shape_swaps_at_midpoint_while_flat() {
        let mut s = ExpressionState::new(neutral());
        let mut wavy = neutral();
        wavy.mouth.shape = MouthShape::Wavy;
        s.set_target(wavy, 0.2, 1.0);
        let f = s.tick(1.0);
        assert_eq!((f.mouth.shape, f.mouth_flat), (MouthShape::Line, 0.0));
        let f = s.tick(1.05);
        assert_eq!(f.mouth.shape, MouthShape::Line);
        assert!(f.mouth_flat > 0.0 && f.mouth_flat < 1.0);
        let f = s.tick(1.1);
        assert_eq!((f.mouth.shape, f.mouth_flat), (MouthShape::Wavy, 1.0));
        let f = s.tick(1.2);
        assert_eq!((f.mouth.shape, f.mouth_flat), (MouthShape::Wavy, 0.0));
        assert!(!s.animating(1.2));
    }

    #[test]
    fn mouth_change_during_unflatten_waits_for_next_swap() {
        let mut s = ExpressionState::new(neutral());
        let mut target = neutral();
        target.mouth.shape = MouthShape::Wavy;
        s.set_target(target, 0.2, 0.0);
        s.tick(0.12); // 已过中点，显示 wavy
        target.mouth.shape = MouthShape::Cat;
        s.set_target(target, 0.2, 0.12);
        // 展开阶段不换形状
        assert_eq!(s.tick(0.15).mouth.shape, MouthShape::Wavy);
        s.tick(0.2); // 这次结束，另起一次
        assert_eq!(s.tick(0.28).mouth.shape, MouthShape::Wavy);
        assert_eq!(s.tick(0.31).mouth.shape, MouthShape::Cat);
    }

    #[test]
    fn eye_change_asks_for_one_blink_and_swaps_at_given_time() {
        let mut s = ExpressionState::new(neutral());
        let smile = Expression {
            eyes: Eyes::Smile,
            ..neutral()
        };
        assert!(s.set_target(smile, 0.2, 0.0));
        s.swap_eyes_at(0.06);
        assert_eq!(s.tick(0.05).eyes, Eyes::Dot);
        // 等待中再换眼型：不再要求眨眼，到时直接换成最新目标
        let wide = Expression {
            eyes: Eyes::Wide,
            ..neutral()
        };
        assert!(!s.set_target(wide, 0.2, 0.05));
        assert_eq!(s.tick(0.06).eyes, Eyes::Wide);
    }

    #[test]
    fn zero_duration_switches_everything_at_once() {
        let mut s = ExpressionState::new(neutral());
        let mut target = with_blush(1.0);
        target.eyes = Eyes::Sad;
        target.mouth.shape = MouthShape::Cat;
        assert!(!s.set_target(target, 0.0, 0.0));
        let f = s.tick(0.0);
        assert_eq!(
            (f.eyes, f.mouth.shape, f.blush),
            (Eyes::Sad, MouthShape::Cat, 1.0)
        );
        assert!(!s.animating(0.0));
    }

    #[test]
    fn patch_validation_names_the_bad_field() {
        let patch = ExpressionPatch {
            mouth: Some(MouthPatch {
                width: Some(1.5),
                ..Default::default()
            }),
            ..Default::default()
        };
        let err = patch.validate().unwrap_err();
        assert!(err.contains("mouth.width") && err.contains("1.5"), "{err}");
    }

    #[test]
    fn complete_needs_every_field() {
        let mut patch: ExpressionPatch = serde_json::from_str(
            r#"{ "eyes": "dot", "mouth": { "shape": "line", "width": 0.35, "open": 0, "curve": 0.2 },
                 "blush": 0, "tears": 0, "sweat": 0, "gloom": 0 }"#,
        )
        .unwrap();
        assert_eq!(patch.complete(), Some(neutral()));
        patch.gloom = None;
        assert_eq!(patch.complete(), None);
    }
}
