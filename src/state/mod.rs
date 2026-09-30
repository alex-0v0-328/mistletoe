//! 纯逻辑状态：表情、眨眼、动作、鼠标跟随、颜文字和标签。
//! 不碰 GPU、不做 I/O、不读时钟：时间 `now`（秒，从程序启动算起）由调用方传入，方便单元测试。
//! 对外只有三个入口：apply（执行命令）、tick（推进到某一时刻并给出快照）、set_pointer（鼠标位置）。

mod blink;
mod command;
mod expression;
mod follow;
mod motion;
mod tags;

use serde::Serialize;

use blink::Blinker;
pub use command::{Command, Reply};
use expression::{Expression, ExpressionState, Face};
use follow::Follow;
use motion::{Motion, MotionReport};
pub use tags::Tables;

/// 没给 duration_ms 时的表情过渡时长，以及允许的上限（毫秒）
const DEFAULT_DURATION_MS: u32 = 200;
const MAX_DURATION_MS: u32 = 10_000;
/// 颜文字最多多少个字符（Unicode 标量值，组合符号也算一个）
const MAX_KAOMOJI_CHARS: usize = 64;
/// 颜文字出现时放大进场的时长（秒）
const KAOMOJI_IN: f64 = 0.18;

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

/// 颜文字在这一帧的样子
#[derive(Clone, Debug, PartialEq)]
pub struct KaomojiView {
    pub text: String,
    /// 进场缩放 0..1（没有淡入，只有放大）
    pub scale: f32,
}

/// 每帧交给 render 的只读结果。
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub pose: Pose,
    pub face: Face,
    /// Some 时颜文字替代眼睛和嘴；腮红、眼泪、汗、阴沉脸照常显示
    pub kaomoji: Option<KaomojiView>,
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

struct Kaomoji {
    text: String,
    since: f64,
}

pub struct State {
    tables: Tables,
    follow: Follow,
    expression: ExpressionState,
    blink: Blinker,
    motion: Motion,
    kaomoji: Option<Kaomoji>,
    /// 最近一帧显示的脸和姿态，get_state 报告的就是它们：用户看到什么，agent 就拿到什么
    shown_face: Face,
    shown_pose: Pose,
}

impl State {
    /// 从 neutral 表情开始。seed 决定随机眨眼的节奏。
    pub fn new(tables: Tables, seed: u64, now: f64) -> Self {
        let mut expression = ExpressionState::new(tables.neutral());
        let shown_face = expression.tick(now);
        Self {
            tables,
            follow: Follow::default(),
            expression,
            blink: Blinker::new(seed, now),
            motion: Motion::default(),
            kaomoji: None,
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
        let blink = self.blink.tick(now);
        let face = Face {
            blink,
            ..self.expression.tick(now)
        };
        let pose = self.follow.tick(now).plus(self.motion.tick(now));
        let kaomoji = self.kaomoji.as_ref().map(|k| KaomojiView {
            text: k.text.clone(),
            scale: smoothstep(((now - k.since) / KAOMOJI_IN) as f32),
        });
        let kaomoji_entering = self
            .kaomoji
            .as_ref()
            .is_some_and(|k| now < k.since + KAOMOJI_IN);
        let animating = !self.follow.settled()
            || self.blink.active()
            || self.expression.animating(now)
            || self.motion.active()
            || kaomoji_entering;
        self.shown_face = face;
        self.shown_pose = pose;
        Snapshot {
            pose,
            face,
            kaomoji,
            animating,
            next_wakeup: self.blink.next_wakeup(),
        }
    }

    /// 所有检查都在修改状态之前做完，保证出错时什么都没变。
    fn try_apply(&mut self, cmd: Command, now: f64) -> Result<Reply, String> {
        match cmd {
            Command::SetTag { tag } => {
                let (expression, motion) = self.resolve_tag(&tag)?;
                self.kaomoji = None;
                self.apply_tag(expression, motion, now);
            }
            Command::SetExpression(args) => {
                let patch = args.patch();
                patch.validate()?;
                let duration = duration_seconds(args.duration_ms)?;
                let base = match &args.preset {
                    Some(name) => self.tables.preset(name)?,
                    None => self.expression.target(),
                };
                self.kaomoji = None;
                self.set_expression(patch.apply_to(base), duration, now);
            }
            Command::SetKaomoji { text, tag } => {
                check_kaomoji(&text)?;
                let resolved = match &tag {
                    Some(name) => Some(self.resolve_tag(name)?),
                    None => None,
                };
                let same_text = self.kaomoji.as_ref().is_some_and(|k| k.text == text);
                if text.is_empty() {
                    self.kaomoji = None;
                } else if !same_text {
                    self.kaomoji = Some(Kaomoji { text, since: now });
                }
                if let Some((expression, motion)) = resolved {
                    self.apply_tag(expression, motion, now);
                }
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

    fn resolve_tag(
        &self,
        name: &str,
    ) -> Result<(Option<Expression>, Option<motion::MotionSpec>), String> {
        let tag = self.tables.tag(name)?;
        let expression = match &tag.expression {
            Some(preset) => Some(self.tables.preset(preset)?),
            None => None,
        };
        Ok((expression, tag.motion))
    }

    /// 标签：有表情就换表情，有动作就播动作；没写的部分保持不变。
    fn apply_tag(
        &mut self,
        expression: Option<Expression>,
        motion: Option<motion::MotionSpec>,
        now: f64,
    ) {
        if let Some(target) = expression {
            self.set_expression(target, DEFAULT_DURATION_MS as f64 / 1000.0, now);
        }
        if let Some(spec) = motion {
            self.motion.play(spec, now);
        }
    }

    /// 设表情目标；眼型要换时安排一次眨眼，在眼睛完全闭上的那一刻换。
    fn set_expression(&mut self, target: Expression, duration: f64, now: f64) {
        if self.expression.set_target(target, duration, now) {
            let closed_at = self.blink.request(now);
            self.expression.swap_eyes_at(closed_at);
        }
    }

    fn report(&self, now: f64) -> StateReport {
        StateReport {
            target: self.expression.target(),
            current: self.shown_face,
            kaomoji: self.kaomoji.as_ref().map(|k| k.text.clone()),
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

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[cfg(test)]
mod tests {
    use super::expression::{Eyes, MouthShape};
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
        assert_eq!(s.expression.target().eyes, Eyes::Smile);
        assert_eq!(
            s.motion.report(1.1).map(|m| m.spec.name),
            Some(MotionName::Bounce)
        );
        // 只有动作的标签不动表情
        ok(&mut s, r#"{"cmd":"set_tag","tag":"no"}"#, 2.0);
        assert_eq!(s.expression.target().eyes, Eyes::Smile);
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
        let t = s.expression.target();
        assert_eq!((t.blush, t.mouth.width), (1.0, 0.6));
        // 带 preset 时从 neutral + 预设重新开始，再叠加字段
        ok(
            &mut s,
            r#"{"cmd":"set_expression","preset":"smile","tears":0.5}"#,
            0.2,
        );
        let t = s.expression.target();
        assert_eq!((t.eyes, t.blush, t.tears), (Eyes::Smile, 0.0, 0.5));
    }

    #[test]
    fn invalid_commands_change_nothing() {
        let mut s = state();
        let before = s.expression.target();
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
        assert_eq!(s.expression.target(), before);
        assert!(s.kaomoji.is_none());
        assert!(!s.motion.active());
    }

    #[test]
    fn eye_shape_swaps_only_while_eyes_are_closed() {
        let mut s = state();
        ok(&mut s, r#"{"cmd":"set_tag","tag":"happy"}"#, 1.0);
        let f = s.tick(1.03).face;
        assert_eq!(f.eyes, Eyes::Dot);
        assert!(f.blink > 0.0 && f.blink < 1.0);
        let f = s.tick(1.06).face;
        assert_eq!(f.eyes, Eyes::Smile);
        assert!(f.blink > 0.99, "换眼型时眼睛应该是闭着的：{}", f.blink);
        let f = s.tick(1.2).face;
        assert_eq!((f.eyes, f.blink), (Eyes::Smile, 0.0));
    }

    #[test]
    fn transition_starts_on_the_next_frame() {
        let mut s = state();
        s.tick(1.0);
        ok(
            &mut s,
            r#"{"cmd":"set_expression","blush":1,"mouth":{"shape":"cat"}}"#,
            1.0,
        );
        let f = s.tick(1.0 + 1.0 / 60.0).face;
        assert!(f.blush > 0.0 && f.blush < 1.0);
        assert!(f.mouth_flat > 0.0);
        let f = s.tick(1.3).face;
        assert_eq!(
            (f.blush, f.mouth.shape, f.mouth_flat),
            (1.0, MouthShape::Cat, 0.0)
        );
    }

    #[test]
    fn kaomoji_keeps_overlays_and_native_commands_leave_it() {
        let mut s = state();
        ok(
            &mut s,
            r#"{"cmd":"set_kaomoji","text":"(•̀ᴗ•́)","tag":"scared"}"#,
            0.0,
        );
        let snap = s.tick(0.09);
        let k = snap.kaomoji.unwrap();
        assert_eq!(k.text, "(•̀ᴗ•́)");
        assert!(k.scale > 0.0 && k.scale < 1.0);
        assert_eq!(s.expression.target().sweat, 1.0);
        assert_eq!(s.tick(1.0).kaomoji.map(|k| k.scale), Some(1.0));
        // 同样的文字再发一次不重新进场
        ok(&mut s, r#"{"cmd":"set_kaomoji","text":"(•̀ᴗ•́)"}"#, 1.0);
        assert_eq!(s.tick(1.01).kaomoji.map(|k| k.scale), Some(1.0));
        ok(&mut s, r#"{"cmd":"set_tag","tag":"happy"}"#, 2.0);
        assert!(s.tick(2.0).kaomoji.is_none());
        ok(&mut s, r#"{"cmd":"set_kaomoji","text":"(^_^)"}"#, 3.0);
        ok(&mut s, r#"{"cmd":"set_kaomoji","text":""}"#, 3.1);
        assert!(s.tick(3.1).kaomoji.is_none());
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
        assert_eq!(s.expression.target(), s.tables.neutral());
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

        let Reply::Tags(t) = run(&mut s, r#"{"cmd":"list_tags"}"#, 0.3) else {
            panic!("list_tags 应该返回标签表");
        };
        let json = serde_json::to_value(&*t).unwrap();
        assert_eq!(json["tags"]["scared"]["motion"]["name"], "tremble");
        assert_eq!(json["eyes"].as_array().map(Vec::len), Some(6));
        assert_eq!(json["motions"].as_array().map(Vec::len), Some(6));
        assert!(
            json["presets"]
                .as_array()
                .unwrap()
                .contains(&"neutral".into())
        );
    }
}
