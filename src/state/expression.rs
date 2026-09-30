//! 表情：眼型、嘴型、叠加层的数据结构，以及“部分表情”的合并与校验。
//! 不管过渡动画：换表情时整张脸怎么压扁、弹开由 swap.rs 负责，这里只描述“脸长什么样”。

use serde::{Deserialize, Serialize};

use super::check_range;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Eyes {
    Dot,
    Smile,
    Closed,
    Squint,
    Wide,
    Sad,
    /// 半睁眼：平直的上眼皮压住眼睛，眉毛往中间下斜
    Annoyed,
}

impl Eyes {
    pub const ALL: [Eyes; 7] = [
        Eyes::Dot,
        Eyes::Smile,
        Eyes::Closed,
        Eyes::Squint,
        Eyes::Wide,
        Eyes::Sad,
        Eyes::Annoyed,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MouthShape {
    Line,
    Cat,
    Triangle,
    Wavy,
    /// 露齿笑：D 形开口里一排牙
    Grin,
}

impl MouthShape {
    pub const ALL: [MouthShape; 5] = [
        MouthShape::Line,
        MouthShape::Cat,
        MouthShape::Triangle,
        MouthShape::Wavy,
        MouthShape::Grin,
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

/// 完整表情。叠加层（腮红、眼泪、汗、阴沉脸、鼻涕泡）都是 0..1 的显现程度。
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Expression {
    pub eyes: Eyes,
    pub mouth: Mouth,
    pub blush: f32,
    pub tears: f32,
    pub sweat: f32,
    pub gloom: f32,
    pub bubble: f32,
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
    pub bubble: Option<f32>,
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
        check_range("gloom", self.gloom, 0.0, 1.0)?;
        check_range("bubble", self.bubble, 0.0, 1.0)
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
            bubble: self.bubble.unwrap_or(base.bubble),
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
            bubble: self.bubble?,
        })
    }
}

/// 这一帧实际显示的脸：交给 render 画。
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Face {
    pub expression: Expression,
    /// 眨眼闭合程度 0（睁开）..1（闭上）
    pub blink: f32,
    /// 整张脸的竖直缩放：平时 1，换表情时压扁到 0 再弹开（会略超过 1）
    pub squish: f32,
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
            bubble: 0.0,
        }
    }

    #[test]
    fn patch_overrides_only_given_fields() {
        let patch: ExpressionPatch =
            serde_json::from_str(r#"{ "eyes": "wide", "mouth": { "open": 0.5 }, "blush": 1 }"#)
                .unwrap();
        let e = patch.apply_to(neutral());
        assert_eq!((e.eyes, e.mouth.open, e.blush), (Eyes::Wide, 0.5, 1.0));
        assert_eq!(
            (e.mouth.shape, e.mouth.width, e.tears),
            (MouthShape::Line, 0.35, 0.0)
        );
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
                 "blush": 0, "tears": 0, "sweat": 0, "gloom": 0, "bubble": 0 }"#,
        )
        .unwrap();
        assert_eq!(patch.complete(), Some(neutral()));
        patch.bubble = None;
        assert_eq!(patch.complete(), None);
    }
}
