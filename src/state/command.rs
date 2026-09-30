//! API 命令和回复的数据结构。命令 JSON 形如 {"cmd": "set_tag", "tag": "happy"}，由 serde 直接解析。
//! 只定义数据，不执行命令（执行在 State::apply）。

use serde::Deserialize;

use super::StateReport;
use super::expression::{ExpressionPatch, Eyes, MouthPatch};
use super::motion::MotionName;
use super::tags::TagsReport;

/// 一条命令。未知字段、未知命令、类型不对都是解析错误。
#[derive(Debug, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    SetTag { tag: String },
    SetExpression(ExpressionArgs),
    SetKaomoji { text: String, tag: Option<String> },
    PlayMotion(MotionArgs),
    // 空结构体而不是单元变体：这样多带字段也会报错
    Blink {},
    GetState {},
    ListTags {},
}

/// set_expression 的参数：可选的预设名 + 要改的字段 + 过渡时长
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpressionArgs {
    pub preset: Option<String>,
    pub eyes: Option<Eyes>,
    pub mouth: Option<MouthPatch>,
    pub blush: Option<f32>,
    pub tears: Option<f32>,
    pub sweat: Option<f32>,
    pub gloom: Option<f32>,
    pub bubble: Option<f32>,
    pub duration_ms: Option<u32>,
}

impl ExpressionArgs {
    pub fn patch(&self) -> ExpressionPatch {
        ExpressionPatch {
            eyes: self.eyes,
            mouth: self.mouth,
            blush: self.blush,
            tears: self.tears,
            sweat: self.sweat,
            gloom: self.gloom,
            bubble: self.bubble,
        }
    }
}

/// play_motion 的参数；没给的字段用 presets.json 里的默认值
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MotionArgs {
    pub motion: MotionName,
    pub duration_ms: Option<u32>,
    pub intensity: Option<f32>,
    #[serde(rename = "loop")]
    pub looping: Option<bool>,
}

/// 命令的结果。api 把它包成 {"ok":true,"result":...} 或 {"ok":false,"error":...}。
#[derive(Debug)]
pub enum Reply {
    /// 执行成功，没有要返回的数据
    Done,
    State(Box<StateReport>),
    Tags(Box<TagsReport>),
    /// 中文错误信息；出错时状态保持不变
    Error(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Result<Command, String> {
        serde_json::from_str(json).map_err(|e| e.to_string())
    }

    #[test]
    fn every_command_parses() {
        assert!(matches!(
            parse(r#"{"cmd":"set_tag","tag":"happy"}"#),
            Ok(Command::SetTag { tag }) if tag == "happy"
        ));
        let Ok(Command::SetExpression(args)) = parse(
            r#"{"cmd":"set_expression","preset":"smile","eyes":"wide","mouth":{"width":0.5},"blush":1,"duration_ms":300}"#,
        ) else {
            panic!("set_expression 解析失败");
        };
        assert_eq!(args.patch().mouth.and_then(|m| m.width), Some(0.5));
        assert_eq!((args.eyes, args.duration_ms), (Some(Eyes::Wide), Some(300)));
        assert!(matches!(
            parse(r#"{"cmd":"set_kaomoji","text":"(•̀ᴗ•́)","tag":"happy"}"#),
            Ok(Command::SetKaomoji { text, tag: Some(_) }) if text == "(•̀ᴗ•́)"
        ));
        let Ok(Command::PlayMotion(m)) =
            parse(r#"{"cmd":"play_motion","motion":"shake","intensity":1.5,"loop":true}"#)
        else {
            panic!("play_motion 解析失败");
        };
        assert_eq!((m.motion, m.looping), (MotionName::Shake, Some(true)));
        assert!(matches!(parse(r#"{"cmd":"blink"}"#), Ok(Command::Blink {})));
        assert!(matches!(
            parse(r#"{"cmd":"get_state"}"#),
            Ok(Command::GetState {})
        ));
        assert!(matches!(
            parse(r#"{"cmd":"list_tags"}"#),
            Ok(Command::ListTags {})
        ));
    }

    #[test]
    fn unknown_fields_are_errors_for_every_command_shape() {
        for json in [
            r#"{"cmd":"set_tag","tag":"happy","extra":1}"#,
            r#"{"cmd":"set_expression","blush":1,"colour":"red"}"#,
            r#"{"cmd":"set_expression","mouth":{"wide":1}}"#,
            r#"{"cmd":"play_motion","motion":"nod","speed":2}"#,
            r#"{"cmd":"blink","times":2}"#,
        ] {
            let err = parse(json).unwrap_err();
            assert!(err.contains("unknown field"), "{json} → {err}");
        }
    }

    #[test]
    fn bad_commands_and_types_are_errors() {
        assert!(parse(r#"{"cmd":"dance"}"#).is_err());
        assert!(parse(r#"{"tag":"happy"}"#).is_err());
        assert!(parse(r#"{"cmd":"set_tag"}"#).is_err());
        assert!(parse(r#"{"cmd":"play_motion","motion":"spin"}"#).is_err());
        assert!(parse(r#"{"cmd":"set_expression","duration_ms":-5}"#).is_err());
        assert!(parse(r#"{"cmd":"set_expression","eyes":"angry"}"#).is_err());
    }
}
