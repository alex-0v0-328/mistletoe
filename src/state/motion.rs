//! 动作：点头、摇头、左右摆、弹跳、发抖的曲线，以及播放、循环和被替换时的淡出。
//! 输出只是叠加在跟随姿态上的 Pose 偏移；不知道窗口和渲染。

use std::f32::consts::{PI, TAU};

use serde::{Deserialize, Serialize};

use super::{Pose, smoothstep};

/// 被新动作替换时，旧动作淡出的时长（秒）
const FADE_OUT: f64 = 0.12;
const NOD_ANGLE: f32 = 15.0_f32.to_radians();
const SHAKE_ANGLE: f32 = 20.0_f32.to_radians();
const SWAY_ANGLE: f32 = 10.0_f32.to_radians();
/// 弹跳高度（球半径的倍数）
const BOUNCE_HEIGHT: f32 = 0.25;
/// 发抖的位移幅度（球半径的倍数）和歪头幅度
const TREMBLE_SHIFT: f32 = 0.04;
const TREMBLE_ROLL: f32 = 2.0_f32.to_radians();

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MotionName {
    Nod,
    Shake,
    Sway,
    Bounce,
    Tremble,
    /// 不是动作，而是让当前动作淡出；没有曲线，也没有默认值
    Stop,
}

impl MotionName {
    /// 有曲线的五个动作；presets.json 必须给它们每个写默认值
    pub const ANIMATED: [MotionName; 5] = [
        MotionName::Nod,
        MotionName::Shake,
        MotionName::Sway,
        MotionName::Bounce,
        MotionName::Tremble,
    ];
    /// list_tags 列出的全部名字（调试页据此生成按钮）
    pub const ALL: [MotionName; 6] = [
        MotionName::Nod,
        MotionName::Shake,
        MotionName::Sway,
        MotionName::Bounce,
        MotionName::Tremble,
        MotionName::Stop,
    ];

    /// 和 JSON 里的写法一致，用于中文错误信息
    pub fn as_str(self) -> &'static str {
        match self {
            MotionName::Nod => "nod",
            MotionName::Shake => "shake",
            MotionName::Sway => "sway",
            MotionName::Bounce => "bounce",
            MotionName::Tremble => "tremble",
            MotionName::Stop => "stop",
        }
    }
}

/// 一次动作的完整参数（默认值已填好、范围已校验）
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct MotionSpec {
    pub name: MotionName,
    pub duration_ms: u32,
    pub intensity: f32,
    #[serde(rename = "loop")]
    pub looping: bool,
}

/// get_state 里报告的当前动作
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct MotionReport {
    #[serde(flatten)]
    pub spec: MotionSpec,
    /// 当前这一轮的进度 0..1
    pub progress: f32,
}

#[derive(Clone, Copy, Debug)]
struct Playing {
    spec: MotionSpec,
    start: f64,
}

impl Playing {
    /// 当前这一轮的进度 0..1；非循环动作播完后返回 None
    fn progress(&self, now: f64) -> Option<f32> {
        let t = ((now - self.start) * 1000.0 / self.spec.duration_ms as f64).max(0.0);
        if self.spec.looping {
            Some(t.fract() as f32)
        } else if t < 1.0 {
            Some(t as f32)
        } else {
            None
        }
    }

    fn pose(&self, now: f64) -> Option<Pose> {
        self.progress(now).map(|p| curve(self.spec, p))
    }
}

/// 动作播放器：同一时刻只有一个当前动作，被替换的动作在淡出列表里逐渐消失。
#[derive(Default)]
pub struct Motion {
    current: Option<Playing>,
    /// 被替换的动作和开始淡出的时刻
    fading: Vec<(Playing, f64)>,
}

impl Motion {
    /// 播放新动作，替换当前动作。新曲线从 0 开始、旧动作淡出，所以衔接处不会跳。
    /// stop 只让当前动作淡出，不播新的。
    pub fn play(&mut self, spec: MotionSpec, now: f64) {
        if let Some(old) = self.current.take()
            && old.progress(now).is_some()
        {
            self.fading.push((old, now));
        }
        if spec.name != MotionName::Stop {
            self.current = Some(Playing { spec, start: now });
        }
    }

    /// 推进到 now，返回所有动作叠加后的姿态偏移。
    pub fn tick(&mut self, now: f64) -> Pose {
        self.fading.retain(|(_, since)| now < since + FADE_OUT);
        let mut pose = Pose::default();
        for (playing, since) in &self.fading {
            let weight = 1.0 - smoothstep(((now - since) / FADE_OUT) as f32);
            if let Some(p) = playing.pose(now) {
                pose = pose.plus(p.scaled(weight));
            }
        }
        match self.current.and_then(|playing| playing.pose(now)) {
            Some(p) => pose = pose.plus(p),
            None => self.current = None,
        }
        pose
    }

    pub fn active(&self) -> bool {
        self.current.is_some() || !self.fading.is_empty()
    }

    pub fn report(&self, now: f64) -> Option<MotionReport> {
        let playing = self.current?;
        Some(MotionReport {
            spec: playing.spec,
            progress: playing.progress(now)?,
        })
    }
}

/// 一轮动作在进度 p（0..1）处的姿态偏移。
/// 每条曲线在 p = 0 和 p = 1 处都是 0，所以开始、结束和循环衔接都不会跳。
fn curve(spec: MotionSpec, p: f32) -> Pose {
    let a = spec.intensity;
    match spec.name {
        // 低头两次：sin² 在 0、0.5、1 处为 0，而且速度也为 0
        MotionName::Nod => Pose {
            pitch: -a * NOD_ANGLE * (TAU * p).sin().powi(2),
            ..Pose::default()
        },
        // 左右各摆两次，乘 sin(πp) 包络让首尾柔和
        MotionName::Shake => Pose {
            yaw: a * SHAKE_ANGLE * (2.0 * TAU * p).sin() * (PI * p).sin(),
            ..Pose::default()
        },
        // 先歪向一边，再歪向另一边
        MotionName::Sway => Pose {
            roll: a * SWAY_ANGLE * (TAU * p).sin(),
            ..Pose::default()
        },
        // 跳两下：|sin| 在落地处是尖角，像真的碰到了地面
        MotionName::Bounce => Pose {
            y: a * BOUNCE_HEIGHT * (TAU * p).sin().abs(),
            ..Pose::default()
        },
        MotionName::Tremble => tremble(spec, p),
        // play 从不把 stop 放进播放列表，这里只为让 match 完整
        MotionName::Stop => Pose::default(),
    }
}

/// 发抖：几个频率不同的正弦叠加，是确定性的，不用随机数。
/// 每个频率都取成“这一轮里整数个周期”，所以 p = 0 和 1 处都是 0，循环时无缝衔接。
fn tremble(spec: MotionSpec, p: f32) -> Pose {
    let a = spec.intensity;
    let seconds = spec.duration_ms as f32 / 1000.0;
    // 频率都低于 30 Hz：60 fps 下不会混叠成慢吞吞的摆动
    let wave = |hz: f32| (TAU * p * (hz * seconds).round().max(1.0)).sin();
    Pose {
        x: a * TREMBLE_SHIFT * (wave(13.0) + 0.5 * wave(19.0)) / 1.5,
        y: a * TREMBLE_SHIFT * 0.6 * wave(17.0),
        roll: a * TREMBLE_ROLL * wave(11.0),
        ..Pose::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(name: MotionName, duration_ms: u32, looping: bool) -> MotionSpec {
        MotionSpec {
            name,
            duration_ms,
            intensity: 2.0,
            looping,
        }
    }

    fn size(p: Pose) -> f32 {
        p.yaw.abs() + p.pitch.abs() + p.roll.abs() + p.x.abs() + p.y.abs()
    }

    #[test]
    fn every_curve_starts_and_ends_at_zero() {
        for name in MotionName::ANIMATED {
            for duration_ms in [300, 600, 1234, 3000] {
                let s = spec(name, duration_ms, false);
                assert!(size(curve(s, 0.0)) < 1e-4, "{name:?} 开头");
                assert!(size(curve(s, 1.0)) < 1e-4, "{name:?} {duration_ms} 结尾");
                assert!(size(curve(s, 0.3)) > 1e-3, "{name:?} 中途应该在动");
            }
        }
    }

    #[test]
    fn one_shot_motion_ends_and_stops_animating() {
        let mut m = Motion::default();
        m.play(spec(MotionName::Nod, 600, false), 1.0);
        assert!(size(m.tick(1.15)) > 0.0);
        assert_eq!(
            m.report(1.3).map(|r| (r.progress * 100.0).round()),
            Some(50.0)
        );
        assert_eq!(m.tick(1.6), Pose::default());
        assert!(!m.active());
        assert!(m.report(1.6).is_none());
    }

    #[test]
    fn looping_motion_wraps_around() {
        let mut m = Motion::default();
        m.play(spec(MotionName::Sway, 1000, true), 0.0);
        let a = m.tick(0.25);
        let b = m.tick(10.25);
        assert!((a.roll - b.roll).abs() < 1e-4);
        assert!(m.active());
    }

    #[test]
    fn replacing_blends_the_old_motion_out_without_a_jump() {
        let mut m = Motion::default();
        m.play(spec(MotionName::Sway, 1000, true), 0.0);
        let before = m.tick(0.25);
        m.play(spec(MotionName::Nod, 600, false), 0.25);
        // 替换的那一刻：旧动作权重还是 1，新动作从 0 开始
        let at = m.tick(0.25);
        assert!(size(at.plus(before.scaled(-1.0))) < 1e-5);
        // 淡出结束后只剩新动作
        let later = m.tick(0.25 + FADE_OUT + 0.01);
        assert!(later.roll.abs() < 1e-6 && later.pitch < 0.0);
        assert_eq!(m.fading.len(), 0);
    }

    #[test]
    fn stop_fades_the_current_motion_out() {
        let mut m = Motion::default();
        m.play(spec(MotionName::Tremble, 1200, true), 0.0);
        let before = m.tick(0.1);
        m.play(spec(MotionName::Stop, 0, false), 0.1);
        // stop 的那一刻不跳，之后淡出到 0
        assert!(size(m.tick(0.1).plus(before.scaled(-1.0))) < 1e-5);
        assert!(m.report(0.1).is_none());
        assert_eq!(m.tick(0.1 + FADE_OUT), Pose::default());
        assert!(!m.active());
    }

    #[test]
    fn quick_double_replace_keeps_both_old_motions_fading() {
        let mut m = Motion::default();
        m.play(spec(MotionName::Sway, 1000, true), 0.0);
        m.play(spec(MotionName::Shake, 600, false), 0.2);
        m.play(spec(MotionName::Nod, 600, false), 0.25);
        m.tick(0.26);
        assert_eq!(m.fading.len(), 2);
    }
}
