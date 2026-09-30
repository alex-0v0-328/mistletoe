//! 预设表情和标签表：解析 presets.json / tags.json，校验引用和数值，给出完整的表情和动作参数。
//! 只接收 JSON 文本，不读文件（读文件和热重载在 app）。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::check_range;
use super::expression::{Expression, ExpressionPatch, Eyes, MouthShape};
use super::motion::{MotionName, MotionSpec};

/// 动作时长的允许范围（毫秒）
const MIN_MOTION_MS: u32 = 50;
const MAX_MOTION_MS: u32 = 60_000;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PresetsFile {
    expressions: BTreeMap<String, ExpressionPatch>,
    motions: BTreeMap<MotionName, MotionDefaults>,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
struct MotionDefaults {
    duration_ms: u32,
    intensity: f32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TagEntry {
    expression: Option<String>,
    motion: Option<TagMotion>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TagMotion {
    name: MotionName,
    duration_ms: Option<u32>,
    intensity: Option<f32>,
    #[serde(rename = "loop")]
    looping: Option<bool>,
}

/// 解析好的标签：表情预设名 + 默认值已填好的动作
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Tag {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expression: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub motion: Option<MotionSpec>,
}

/// list_tags 的结果；调试页用它生成按钮
#[derive(Debug, Serialize)]
pub struct TagsReport {
    pub tags: BTreeMap<String, Tag>,
    pub presets: Vec<String>,
    pub eyes: &'static [Eyes],
    pub mouths: &'static [MouthShape],
    pub motions: &'static [MotionName],
}

pub struct Tables {
    neutral: Expression,
    /// 所有预设，都已叠加在 neutral 上（包括 neutral 本身），所以同一个标签每次看起来都一样
    presets: BTreeMap<String, Expression>,
    motions: BTreeMap<MotionName, MotionDefaults>,
    tags: BTreeMap<String, Tag>,
}

impl Tables {
    /// 解析并校验两个文件。任何一处出错都整体拒绝，调用方保留旧表。
    pub fn parse(presets_json: &str, tags_json: &str) -> Result<Self, String> {
        let file: PresetsFile =
            serde_json::from_str(presets_json).map_err(|e| json_error("presets.json", &e))?;
        let neutral = file
            .expressions
            .get("neutral")
            .ok_or("presets.json 缺少 neutral 预设")?
            .complete()
            .ok_or("presets.json 的 neutral 必须写全所有字段：eyes、mouth 的 shape/width/open/curve、blush、tears、sweat、gloom、bubble")?;
        let mut presets = BTreeMap::new();
        for (name, patch) in &file.expressions {
            check_name("预设", name).map_err(|e| format!("presets.json：{e}"))?;
            patch
                .validate()
                .map_err(|e| format!("presets.json 预设 {name}：{e}"))?;
            presets.insert(name.clone(), patch.apply_to(neutral));
        }
        if file.motions.contains_key(&MotionName::Stop) {
            return Err("presets.json：stop 不是动作，不需要默认值".to_string());
        }
        for name in MotionName::ANIMATED {
            let d = file
                .motions
                .get(&name)
                .ok_or_else(|| format!("presets.json 缺少动作 {} 的默认值", name.as_str()))?;
            validate_motion(d.duration_ms, d.intensity)
                .map_err(|e| format!("presets.json 动作 {}：{e}", name.as_str()))?;
        }
        let mut tables = Tables {
            neutral,
            presets,
            motions: file.motions,
            tags: BTreeMap::new(),
        };

        let entries: BTreeMap<String, TagEntry> =
            serde_json::from_str(tags_json).map_err(|e| json_error("tags.json", &e))?;
        for (name, entry) in entries {
            check_name("标签", &name).map_err(|e| format!("tags.json：{e}"))?;
            if let Some(preset) = &entry.expression
                && !tables.presets.contains_key(preset)
            {
                return Err(format!("tags.json 标签 {name} 引用了不存在的预设 {preset}"));
            }
            let motion = match entry.motion {
                Some(m) => Some(
                    tables
                        .motion(m.name, m.duration_ms, m.intensity, m.looping)
                        .map_err(|e| format!("tags.json 标签 {name}：{e}"))?,
                ),
                None => None,
            };
            let tag = Tag {
                expression: entry.expression,
                motion,
            };
            tables.tags.insert(name, tag);
        }
        Ok(tables)
    }

    pub fn neutral(&self) -> Expression {
        self.neutral
    }

    pub fn preset(&self, name: &str) -> Result<Expression, String> {
        self.presets
            .get(name)
            .copied()
            .ok_or_else(|| format!("没有叫 {name} 的预设"))
    }

    pub fn tag(&self, name: &str) -> Result<&Tag, String> {
        self.tags
            .get(name)
            .ok_or_else(|| format!("没有叫 {name} 的标签"))
    }

    /// 用 presets.json 里的默认值补全动作参数并校验范围。loop 默认 false（只播一次）。
    /// stop 不接受任何参数。
    pub fn motion(
        &self,
        name: MotionName,
        duration_ms: Option<u32>,
        intensity: Option<f32>,
        looping: Option<bool>,
    ) -> Result<MotionSpec, String> {
        if name == MotionName::Stop {
            if duration_ms.is_some() || intensity.is_some() || looping.is_some() {
                return Err("stop 不接受 duration_ms、intensity、loop".to_string());
            }
            return Ok(MotionSpec {
                name,
                duration_ms: 0,
                intensity: 0.0,
                looping: false,
            });
        }
        let defaults = self
            .motions
            .get(&name)
            .ok_or_else(|| format!("缺少动作 {} 的默认值", name.as_str()))?;
        let spec = MotionSpec {
            name,
            duration_ms: duration_ms.unwrap_or(defaults.duration_ms),
            intensity: intensity.unwrap_or(defaults.intensity),
            looping: looping.unwrap_or(false),
        };
        validate_motion(spec.duration_ms, spec.intensity)?;
        Ok(spec)
    }

    pub fn report(&self) -> TagsReport {
        TagsReport {
            tags: self.tags.clone(),
            presets: self.presets.keys().cloned().collect(),
            eyes: &Eyes::ALL,
            mouths: &MouthShape::ALL,
            motions: &MotionName::ALL,
        }
    }
}

fn validate_motion(duration_ms: u32, intensity: f32) -> Result<(), String> {
    if !(MIN_MOTION_MS..=MAX_MOTION_MS).contains(&duration_ms) {
        return Err(format!(
            "duration_ms 必须在 {MIN_MOTION_MS} 到 {MAX_MOTION_MS} 之间，收到 {duration_ms}"
        ));
    }
    check_range("intensity", Some(intensity), 0.0, 2.0)
}

/// 标签名和预设名只用英文：小写字母、数字、下划线、连字符。
fn check_name(kind: &str, name: &str) -> Result<(), String> {
    let ok = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-');
    if ok {
        Ok(())
    } else {
        Err(format!(
            "{kind}名 {name:?} 只能用小写英文字母、数字、下划线和连字符"
        ))
    }
}

/// serde_json 的错误消息末尾自带英文位置 " at line X column Y"，换成中文位置放在前面。
pub fn json_error(source: &str, e: &serde_json::Error) -> String {
    let message = e.to_string();
    let message = message.split(" at line ").next().unwrap_or(&message);
    format!("{source} 第 {} 行第 {} 列：{message}", e.line(), e.column())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRESETS: &str = include_str!("../../data/presets.json");
    const TAGS: &str = include_str!("../../data/tags.json");

    #[test]
    fn built_in_data_parses_and_has_the_spec_tags() {
        let t = Tables::parse(PRESETS, TAGS).unwrap();
        assert_eq!(t.tag("happy").unwrap().expression.as_deref(), Some("smile"));
        assert_eq!(
            t.tag("happy").unwrap().motion.map(|m| m.name),
            Some(MotionName::Bounce)
        );
        assert_eq!(
            t.tag("no").unwrap().motion.map(|m| m.name),
            Some(MotionName::Shake)
        );
        let scared = t.tag("scared").unwrap();
        assert_eq!(t.preset("scared").unwrap().sweat, 1.0);
        assert_eq!(
            scared.motion.map(|m| (m.name, m.looping)),
            Some((MotionName::Tremble, true))
        );
    }

    #[test]
    fn presets_sit_on_top_of_neutral() {
        let t = Tables::parse(PRESETS, TAGS).unwrap();
        let smile = t.preset("smile").unwrap();
        assert_eq!(smile.eyes, Eyes::Smile);
        // smile 没写的字段来自 neutral
        assert_eq!(smile.blush, t.neutral().blush);
        assert_eq!(smile.gloom, t.neutral().gloom);
    }

    #[test]
    fn tag_motion_fields_default_from_presets() {
        let t = Tables::parse(PRESETS, TAGS).unwrap();
        let bounce = t.tag("happy").unwrap().motion.unwrap();
        assert_eq!(
            (bounce.duration_ms, bounce.intensity, bounce.looping),
            (700, 1.0, false)
        );
    }

    fn parse_err(presets: &str, tags: &str) -> String {
        match Tables::parse(presets, tags) {
            Ok(_) => panic!("应该报错"),
            Err(e) => e,
        }
    }

    #[test]
    fn bad_references_and_values_are_rejected() {
        let e = parse_err(PRESETS, r#"{ "x": { "expression": "nope" } }"#);
        assert!(e.contains("nope"), "{e}");
        let e = parse_err(
            PRESETS,
            r#"{ "x": { "motion": { "name": "nod", "intensity": 3 } } }"#,
        );
        assert!(e.contains("intensity"), "{e}");
        let e = parse_err(PRESETS, r#"{ "x": { "motion": { "name": "spin" } } }"#);
        assert!(e.contains("tags.json"), "{e}");
        let e = parse_err(PRESETS, r#"{ "开心": { "expression": "smile" } }"#);
        assert!(e.contains("开心"), "{e}");
        let e = parse_err(&PRESETS.replace("\"sway\"", "\"swayy\""), TAGS);
        assert!(e.contains("presets.json"), "{e}");
        let e = parse_err(&PRESETS.replace(", \"bubble\": 0 }", " }"), TAGS);
        assert!(e.contains("neutral"), "{e}");
    }

    #[test]
    fn stop_takes_no_parameters_and_needs_no_defaults() {
        let t = Tables::parse(PRESETS, TAGS).unwrap();
        assert!(t.motion(MotionName::Stop, None, None, None).is_ok());
        let e = t
            .motion(MotionName::Stop, Some(500), None, None)
            .unwrap_err();
        assert!(e.contains("stop"), "{e}");
        let e = parse_err(
            &PRESETS.replace(
                "\"motions\": {",
                "\"motions\": { \"stop\": { \"duration_ms\": 100, \"intensity\": 1 },",
            ),
            TAGS,
        );
        assert!(e.contains("stop"), "{e}");
    }

    #[test]
    fn json_errors_carry_line_and_column_in_chinese() {
        let e = parse_err(PRESETS, "{\n  \"happy\": { \"expression\": \"smile\", }\n}");
        assert!(e.starts_with("tags.json 第 2 行第"), "{e}");
        assert!(!e.contains(" at line "), "{e}");
    }
}
