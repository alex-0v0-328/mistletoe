//! 纯逻辑状态：表情、眨眼、动作、鼠标跟随、颜文字和标签。
//! 不碰 GPU、不做 I/O、不读时钟：时间 `now`（秒，从程序启动算起）由调用方传入，方便单元测试。
//! 对外只有三个入口：apply（执行命令）、tick（推进到某一时刻并给出快照）、set_pointer（鼠标位置）。

mod blink;
mod command;
mod expression;
mod follow;
mod motion;
mod swap;
mod tags;

use serde::Serialize;

use blink::Blinker;
pub use command::{Command, Reply};
use expression::Expression;
pub use expression::{Eyes, Face, MouthShape};
use follow::Follow;
use motion::{Motion, MotionReport, MotionSpec};
use swap::Swap;
pub use tags::Tables;

/// 没给 duration_ms 时换表情动画（压扁 → 换 → 弹开）的时长，以及允许的上限（毫秒）
const DEFAULT_DURATION_MS: u32 = 300;
const MAX_DURATION_MS: u32 = 10_000;
/// 颜文字最多多少个字符（Unicode 标量值，组合符号也算一个）
const MAX_KAOMOJI_CHARS: usize = 64;

/// 指针相对窗口中心的位置，以半个窗口短边为单位：x 向右、y 向上。
/// 指针在窗口外时可以超出 ±1，没有上限。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pointer {
    pub x: f32,
    pub y: f32,
}

/// 球的姿态：跟随和动作叠加后的结果。
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct Pose {
    /// 弧度。yaw 为正 = 脸向右转，pitch 为正 = 抬头，roll 为正 = 逆时针歪头（观众视角）
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
    /// 球心位移，以球半径为单位，x 向右、y 向上
    pub x: f32,
    pub y: f32,
}

impl Pose {
    fn plus(self, o: Pose) -> Pose {
        Pose {
            yaw: self.yaw + o.yaw,
            pitch: self.pitch + o.pitch,
            roll: self.roll + o.roll,
            x: self.x + o.x,
            y: self.y + o.y,
        }
    }

    fn scaled(self, k: f32) -> Pose {
        Pose {
            yaw: self.yaw * k,
            pitch: self.pitch * k,
            roll: self.roll * k,
            x: self.x * k,
            y: self.y * k,
        }
    }
}

/// 每帧交给 render 的只读结果。
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub pose: Pose,
    pub face: Face,
    /// Some 时颜文字替代眼睛和嘴；腮红、眼泪、汗、阴沉脸、鼻涕泡照常显示
    pub kaomoji: Option<String>,
    /// 球的缩放：换表情时轻微放大一下
    pub scale: f32,
    /// 还在动：app 需要继续出帧；为 false 时 app 可以睡到 next_wakeup
    pub animating: bool,
    /// 空闲时下一次需要醒来的时刻（下一次自动眨眼）
    pub next_wakeup: Option<f64>,
}

/// get_state 的结果（frames_rendered、last_latency_ms 由 app 补充）
#[derive(Debug, Serialize)]
pub struct StateReport {
    /// 目标表情
    pub target: Expression,
    /// 最近一帧实际显示的脸
    pub current: Face,
    pub kaomoji: Option<String>,
    pub motion: Option<MotionReport>,
    /// 最近一帧的姿态
    pub pose: Pose,
}

/// 脸上显示的内容：原生表情 + 可选的颜文字。换表情动画在中点把“显示的”一次性换成“目标”。
#[derive(Clone, Debug, PartialEq)]
struct Look {
    expression: Expression,
    kaomoji: Option<String>,
}

pub struct State {
    tables: Tables,
    follow: Follow,
    blink: Blinker,
    motion: Motion,
    swap: Swap,
    target: Look,
    shown: Look,
    /// 最近一次命令给的换表情时长；弹开阶段目标又变时，下一次动画用它
    swap_duration: f64,
    /// 最近一帧显示的脸和姿态，get_state 报告的就是它们：用户看到什么，agent 就拿到什么
    shown_face: Face,
    shown_pose: Pose,
}

impl State {
    /// 从 neutral 表情开始。seed 决定随机眨眼的节奏。
    pub fn new(tables: Tables, seed: u64, now: f64) -> Self {
        let look = Look {
            expression: tables.neutral(),
            kaomoji: None,
        };
        let shown_face = Face {
            expression: look.expression,
            blink: 0.0,
            squish: 1.0,
        };
        Self {
            tables,
            follow: Follow::default(),
            blink: Blinker::new(seed, now),
            motion: Motion::default(),
            swap: Swap::default(),
            target: look.clone(),
            shown: look,
            swap_duration: DEFAULT_DURATION_MS as f64 / 1000.0,
            shown_face,
            shown_pose: Pose::default(),
        }
    }

    /// 更新指针位置。返回 true 表示跟随目标变了，需要重绘。
    pub fn set_pointer(&mut self, pointer: Pointer) -> bool {
        self.follow.set_pointer(pointer)
    }

    /// 执行一条命令。出错时状态完全不变，回复里是中文错误。
    pub fn apply(&mut self, cmd: Command, now: f64) -> Reply {
        match self.try_apply(cmd, now) {
            Ok(reply) => reply,
            Err(e) => Reply::Error(e),
        }
    }

    /// 推进到时间 `now`，返回这一帧的快照。
    pub fn tick(&mut self, now: f64) -> Snapshot {
        let frame = self.swap.tick(now);
        if frame.swap_now {
            // 中点：脸压扁到 0 的这一刻换内容，看不出跳变
            self.shown = self.target.clone();
        }
        if frame.finished && self.shown != self.target {
            // 弹开阶段目标又变了：马上再换一次
            self.swap.start(self.swap_duration, now);
        }
        let face = Face {
            expression: self.shown.expression,
            blink: self.blink.tick(now),
            squish: frame.squish,
        };
        let pose = self.follow.tick(now).plus(self.motion.tick(now));
        let animating = !self.follow.settled()
            || self.blink.active()
            || self.swap.active()
            || self.motion.active();
        self.shown_face = face;
        self.shown_pose = pose;
        Snapshot {
            pose,
            face,
            kaomoji: self.shown.kaomoji.clone(),
            scale: frame.scale,
            animating,
            next_wakeup: self.blink.next_wakeup(),
        }
    }

    /// 所有检查都在修改状态之前做完，保证出错时什么都没变。
    fn try_apply(&mut self, cmd: Command, now: f64) -> Result<Reply, String> {
        let default_duration = DEFAULT_DURATION_MS as f64 / 1000.0;
        match cmd {
            Command::SetTag { tag } => {
                let (expression, motion) = self.resolve_tag(&tag)?;
                let look = Look {
                    expression: expression.unwrap_or(self.target.expression),
                    kaomoji: None,
                };
                self.set_look(look, default_duration, now);
                self.play(motion, now);
            }
            Command::SetExpression(args) => {
                let patch = args.patch();
                patch.validate()?;
                let duration = duration_seconds(args.duration_ms)?;
                let base = match &args.preset {
                    Some(name) => self.tables.preset(name)?,
                    None => self.target.expression,
                };
                let look = Look {
                    expression: patch.apply_to(base),
                    kaomoji: None,
                };
                self.set_look(look, duration, now);
            }
            Command::SetKaomoji { text, tag } => {
                check_kaomoji(&text)?;
                let (expression, motion) = match &tag {
                    Some(name) => self.resolve_tag(name)?,
                    None => (None, None),
                };
                let look = Look {
                    expression: expression.unwrap_or(self.target.expression),
                    kaomoji: (!text.is_empty()).then_some(text),
                };
                self.set_look(look, default_duration, now);
                self.play(motion, now);
            }
            Command::PlayMotion(args) => {
                let spec = self.tables.motion(
                    args.motion,
                    args.duration_ms,
                    args.intensity,
                    args.looping,
                )?;
                self.motion.play(spec, now);
            }
            Command::Blink {} => {
                self.blink.request(now);
            }
            Command::GetState {} => return Ok(Reply::State(Box::new(self.report(now)))),
            Command::ListTags {} => return Ok(Reply::Tags(Box::new(self.tables.report()))),
        }
        Ok(Reply::Done)
    }

    fn resolve_tag(&self, name: &str) -> Result<(Option<Expression>, Option<MotionSpec>), String> {
        let tag = self.tables.tag(name)?;
        let expression = match &tag.expression {
            Some(preset) => Some(self.tables.preset(preset)?),
            None => None,
        };
        Ok((expression, tag.motion))
    }

    fn play(&mut self, motion: Option<MotionSpec>, now: f64) {
        if let Some(spec) = motion {
            self.motion.play(spec, now);
        }
    }

    /// 设新的脸。duration 为 0 立即换；否则整张脸压扁到 0、在中点换、再弹开。
    /// 动画还没到中点时再改目标，中点直接换成最新的；已过中点则等这次结束后再来一次（见 tick）。
    fn set_look(&mut self, look: Look, duration: f64, now: f64) {
        self.target = look;
        if duration <= 0.0 {
            self.shown = self.target.clone();
            self.swap = Swap::default();
            return;
        }
        self.swap_duration = duration;
        if self.target != self.shown {
            self.swap.start(duration, now);
        }
    }

    fn report(&self, now: f64) -> StateReport {
        StateReport {
            target: self.target.expression,
            current: self.shown_face,
            kaomoji: self.target.kaomoji.clone(),
            motion: self.motion.report(now),
            pose: self.shown_pose,
        }
    }
}

fn duration_seconds(duration_ms: Option<u32>) -> Result<f64, String> {
    let ms = duration_ms.unwrap_or(DEFAULT_DURATION_MS);
    if ms > MAX_DURATION_MS {
        return Err(format!(
            "duration_ms 必须在 0 到 {MAX_DURATION_MS} 之间，收到 {ms}"
        ));
    }
    Ok(ms as f64 / 1000.0)
}

fn check_kaomoji(text: &str) -> Result<(), String> {
    let count = text.chars().count();
    if count > MAX_KAOMOJI_CHARS {
        return Err(format!(
            "颜文字最多 {MAX_KAOMOJI_CHARS} 个字符，收到 {count} 个"
        ));
    }
    if text.chars().any(char::is_control) {
        return Err("颜文字只能是单行文字，不能包含换行、制表符等控制字符".to_string());
    }
    Ok(())
}

/// 检查可选数值是否在 [min, max] 内。越界返回中文错误。
fn check_range(name: &str, value: Option<f32>, min: f32, max: f32) -> Result<(), String> {
    match value {
        Some(v) if !(min..=max).contains(&v) => {
            Err(format!("{name} 必须在 {min} 到 {max} 之间，收到 {v}"))
        }
        _ => Ok(()),
    }
}

/// 0..1 的平滑曲线 3x² - 2x³，两端速度为 0。输入先截到 [0, 1]。
fn smoothstep(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

#[cfg(test)]
mod tests {
    use super::motion::MotionName;
    use super::*;

    const PRESETS: &str = include_str!("../../data/presets.json");
    const TAGS: &str = include_str!("../../data/tags.json");

    fn state() -> State {
        State::new(Tables::parse(PRESETS, TAGS).unwrap(), 42, 0.0)
    }

    fn run(s: &mut State, json: &str, now: f64) -> Reply {
        s.apply(serde_json::from_str(json).unwrap(), now)
    }

    fn ok(s: &mut State, json: &str, now: f64) {
        let reply = run(s, json, now);
        assert!(matches!(reply, Reply::Done), "{json} → {reply:?}");
    }

    fn error(s: &mut State, json: &str, now: f64) -> String {
        match run(s, json, now) {
            Reply::Error(e) => e,
            other => panic!("{json} 应该报错，却得到 {other:?}"),
        }
    }

    #[test]
    fn set_tag_applies_preset_and_motion() {
        let mut s = state();
        ok(&mut s, r#"{"cmd":"set_tag","tag":"happy"}"#, 1.0);
        assert_eq!(s.target.expression.eyes, Eyes::Smile);
        assert_eq!(
            s.motion.report(1.1).map(|m| m.spec.name),
            Some(MotionName::Bounce)
        );
        // 只有动作的标签不动表情
        ok(&mut s, r#"{"cmd":"set_tag","tag":"no"}"#, 2.0);
        assert_eq!(s.target.expression.eyes, Eyes::Smile);
        assert_eq!(
            s.motion.report(2.1).map(|m| m.spec.name),
            Some(MotionName::Shake)
        );
    }

    #[test]
    fn set_expression_merges_onto_current_target() {
        let mut s = state();
        ok(&mut s, r#"{"cmd":"set_expression","blush":1}"#, 0.0);
        ok(
            &mut s,
            r#"{"cmd":"set_expression","mouth":{"width":0.6}}"#,
            0.1,
        );
        let t = s.target.expression;
        assert_eq!((t.blush, t.mouth.width), (1.0, 0.6));
        // 带 preset 时从 neutral + 预设重新开始，再叠加字段
        ok(
            &mut s,
            r#"{"cmd":"set_expression","preset":"smile","tears":0.5}"#,
            0.2,
        );
        let t = s.target.expression;
        assert_eq!((t.eyes, t.blush, t.tears), (Eyes::Smile, 0.0, 0.5));
    }

    #[test]
    fn invalid_commands_change_nothing() {
        let mut s = state();
        let before = s.target.clone();
        for (json, needle) in [
            (r#"{"cmd":"set_tag","tag":"dance"}"#, "dance"),
            (
                r#"{"cmd":"set_expression","mouth":{"width":1.5}}"#,
                "mouth.width",
            ),
            (
                r#"{"cmd":"set_expression","preset":"nope","blush":1}"#,
                "nope",
            ),
            (
                r#"{"cmd":"set_expression","blush":1,"duration_ms":99999}"#,
                "duration_ms",
            ),
            (r#"{"cmd":"set_kaomoji","text":"a\nb"}"#, "单行"),
            (
                r#"{"cmd":"set_kaomoji","text":"(^_^)","tag":"dance"}"#,
                "dance",
            ),
            (
                r#"{"cmd":"play_motion","motion":"nod","intensity":3}"#,
                "intensity",
            ),
            (
                r#"{"cmd":"play_motion","motion":"nod","duration_ms":10}"#,
                "duration_ms",
            ),
        ] {
            let e = error(&mut s, json, 1.0);
            assert!(e.contains(needle), "{json} → {e}");
        }
        assert_eq!(s.target, before);
        assert!(!s.swap.active());
        assert!(!s.motion.active());
    }

    #[test]
    fn face_swaps_at_the_midpoint_while_squished_flat() {
        let mut s = state();
        s.tick(1.0);
        ok(&mut s, r#"{"cmd":"set_tag","tag":"happy"}"#, 1.0);
        // 默认 300 ms：前 40%（120 ms）压扁，中点换，之后弹开
        let snap = s.tick(1.06);
        assert_eq!(snap.face.expression.eyes, Eyes::Dot);
        assert!(snap.face.squish > 0.0 && snap.face.squish < 1.0);
        let snap = s.tick(1.12);
        assert_eq!(snap.face.expression.eyes, Eyes::Smile);
        assert!(
            snap.face.squish.abs() < 1e-3,
            "换的那一刻脸是扁的：{}",
            snap.face.squish
        );
        assert!(snap.scale > 1.0, "球轻微放大");
        let snap = s.tick(1.3);
        assert_eq!((snap.face.squish, snap.scale), (1.0, 1.0));
    }

    #[test]
    fn retarget_before_midpoint_swaps_once_after_it_swaps_again() {
        let mut s = state();
        ok(&mut s, r#"{"cmd":"set_tag","tag":"happy"}"#, 0.0);
        s.tick(0.05);
        ok(&mut s, r#"{"cmd":"set_tag","tag":"sad"}"#, 0.05);
        // 还没到中点：中点直接换成最新的 sad
        assert_eq!(s.tick(0.13).face.expression.eyes, Eyes::Sad);
        ok(&mut s, r#"{"cmd":"set_tag","tag":"scared"}"#, 0.2);
        // 已过中点：这次弹开结束后再压扁一次，再换
        assert_eq!(s.tick(0.25).face.expression.eyes, Eyes::Sad);
        s.tick(0.3);
        assert_eq!(s.tick(0.42).face.expression.eyes, Eyes::Wide);
    }

    #[test]
    fn zero_duration_switches_at_once() {
        let mut s = state();
        ok(
            &mut s,
            r#"{"cmd":"set_expression","eyes":"wide","duration_ms":0}"#,
            0.0,
        );
        let snap = s.tick(0.0);
        assert_eq!(
            (snap.face.expression.eyes, snap.face.squish),
            (Eyes::Wide, 1.0)
        );
        assert!(!s.swap.active());
    }

    #[test]
    fn kaomoji_swaps_in_at_the_midpoint_and_native_commands_leave_it() {
        let mut s = state();
        ok(
            &mut s,
            r#"{"cmd":"set_kaomoji","text":"(•̀ᴗ•́)","tag":"scared"}"#,
            0.0,
        );
        assert!(s.tick(0.06).kaomoji.is_none());
        assert_eq!(s.tick(0.13).kaomoji.as_deref(), Some("(•̀ᴗ•́)"));
        // 颜文字只替代眼睛和嘴，叠加层照常
        assert_eq!(s.shown.expression.sweat, 1.0);
        s.tick(0.4);
        // 同样的文字再发一次：没有变化，不重新压扁
        ok(&mut s, r#"{"cmd":"set_kaomoji","text":"(•̀ᴗ•́)"}"#, 1.0);
        assert!(!s.swap.active());
        ok(&mut s, r#"{"cmd":"set_tag","tag":"happy"}"#, 2.0);
        s.tick(2.2);
        assert!(s.tick(2.4).kaomoji.is_none());
        ok(&mut s, r#"{"cmd":"set_kaomoji","text":"(^_^)"}"#, 3.0);
        s.tick(3.4);
        ok(&mut s, r#"{"cmd":"set_kaomoji","text":""}"#, 4.0);
        s.tick(4.2);
        assert!(s.tick(4.4).kaomoji.is_none());
    }

    #[test]
    fn stop_ends_a_looping_motion_and_neutral_tag_calms_down() {
        let mut s = state();
        ok(&mut s, r#"{"cmd":"set_tag","tag":"scared"}"#, 0.0);
        assert!(s.motion.active());
        ok(&mut s, r#"{"cmd":"play_motion","motion":"stop"}"#, 1.0);
        s.tick(1.2);
        assert!(!s.motion.active());
        let e = error(
            &mut s,
            r#"{"cmd":"play_motion","motion":"stop","loop":true}"#,
            1.3,
        );
        assert!(e.contains("stop"), "{e}");
        ok(&mut s, r#"{"cmd":"set_tag","tag":"scared"}"#, 2.0);
        ok(&mut s, r#"{"cmd":"set_tag","tag":"neutral"}"#, 3.0);
        s.tick(3.2);
        assert!(!s.motion.active());
        assert_eq!(s.target.expression, s.tables.neutral());
    }

    #[test]
    fn blink_command_blinks_now() {
        let mut s = state();
        s.tick(1.0);
        ok(&mut s, r#"{"cmd":"blink"}"#, 1.0);
        assert!(s.tick(1.06).face.blink > 0.99);
        assert_eq!(s.tick(1.2).face.blink, 0.0);
    }

    #[test]
    fn settles_to_idle_and_sleeps_until_next_blink() {
        let mut s = state();
        ok(&mut s, r#"{"cmd":"set_tag","tag":"happy"}"#, 0.0);
        let mut now = 0.0;
        let snap = loop {
            let snap = s.tick(now);
            if !snap.animating {
                break snap;
            }
            now += 1.0 / 60.0;
            assert!(now < 10.0, "10 秒内没有停下来");
        };
        let wake = snap.next_wakeup.unwrap();
        assert!(wake > now && wake <= now + 6.0, "{wake} vs {now}");
    }

    #[test]
    fn get_state_and_list_tags_report() {
        let mut s = state();
        ok(&mut s, r#"{"cmd":"play_motion","motion":"shake"}"#, 0.0);
        s.tick(0.3);
        let Reply::State(r) = run(&mut s, r#"{"cmd":"get_state"}"#, 0.3) else {
            panic!("get_state 应该返回状态");
        };
        let m = r.motion.unwrap();
        assert_eq!(
            (m.spec.name, (m.progress * 10.0).round()),
            (MotionName::Shake, 5.0)
        );
        let json = serde_json::to_value(&*r).unwrap();
        assert_eq!(json["motion"]["loop"], false);
        assert_eq!(json["target"]["eyes"], "dot");
        assert_eq!(json["current"]["squish"], 1.0);

        let Reply::Tags(t) = run(&mut s, r#"{"cmd":"list_tags"}"#, 0.3) else {
            panic!("list_tags 应该返回标签表");
        };
        let json = serde_json::to_value(&*t).unwrap();
        assert_eq!(json["tags"]["scared"]["motion"]["name"], "tremble");
        assert_eq!(json["eyes"].as_array().map(Vec::len), Some(7));
        assert_eq!(json["mouths"].as_array().map(Vec::len), Some(5));
        assert_eq!(json["motions"].as_array().map(Vec::len), Some(6));
        assert!(
            json["presets"]
                .as_array()
                .unwrap()
                .contains(&"neutral".into())
        );
    }
}
