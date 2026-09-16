//! Serde types for the Vega-Lite subset we implement.
//!
//! Deliberately **permissive about unknown fields** and **strict about
//! unsupported values**. Real Vega-Lite specs carry `$schema`, `description`,
//! and other keys we do not read; rejecting those would make the library
//! useless for pasting in a spec that works elsewhere. But an unsupported
//! *mark type* is a different matter — silently ignoring it would render a
//! blank chart, so it becomes a typed error.

use serde::Deserialize;

use crate::error::{Error, Result};

/// Vega-Lite's default plot-area size when `width`/`height` are omitted.
pub const DEFAULT_WIDTH: f32 = 200.0;
pub const DEFAULT_HEIGHT: f32 = 200.0;

/// Upper bound on canvas dimensions, to keep a hostile spec from asking for a
/// multi-gigabyte pixmap.
pub const MAX_DIMENSION: f32 = 8192.0;

#[derive(Debug, Clone, Deserialize)]
pub struct Spec {
    #[serde(default)]
    pub width: Option<f32>,
    #[serde(default)]
    pub height: Option<f32>,
    #[serde(default)]
    pub title: Option<TitleDef>,
    #[serde(default)]
    pub background: Option<String>,
    #[serde(default)]
    pub padding: Option<f32>,
    pub data: DataDef,
    pub mark: MarkDef,
    #[serde(default)]
    pub encoding: Encoding,
}

/// `"title": "x"` or `"title": { "text": "x" }`.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum TitleDef {
    Text(String),
    Object {
        #[serde(default)]
        text: Option<String>,
    },
}

impl TitleDef {
    pub fn text(&self) -> Option<&str> {
        match self {
            TitleDef::Text(s) => Some(s.as_str()),
            TitleDef::Object { text } => text.as_deref(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct DataDef {
    #[serde(default)]
    pub values: Option<Vec<serde_json::Value>>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

impl DataDef {
    /// Inline `values` is the only supported source: a Worker render must not
    /// make an outbound fetch as a side effect of drawing a chart.
    pub fn rows(&self) -> Result<&[serde_json::Value]> {
        if self.url.is_some() {
            return Err(Error::UnsupportedData("data.url"));
        }
        if self.values.is_none() && self.name.is_some() {
            return Err(Error::UnsupportedData("named data sources"));
        }
        match &self.values {
            Some(v) => Ok(v.as_slice()),
            None => Err(Error::UnsupportedData("spec has no data.values")),
        }
    }
}

/// `"mark": "bar"` or `"mark": { "type": "bar", ... }`.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum MarkDef {
    Shorthand(String),
    Full(MarkProps),
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkProps {
    #[serde(rename = "type")]
    pub mark_type: String,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub fill: Option<String>,
    #[serde(default)]
    pub stroke: Option<String>,
    #[serde(default)]
    pub opacity: Option<f32>,
    #[serde(default)]
    pub size: Option<f32>,
    #[serde(default)]
    pub stroke_width: Option<f32>,
    #[serde(default)]
    pub filled: Option<bool>,
    /// Hole radius for an `arc` mark. Zero (the default) gives a pie.
    #[serde(default)]
    pub inner_radius: Option<f32>,
    #[serde(default)]
    pub outer_radius: Option<f32>,
}

impl MarkDef {
    pub fn mark_type(&self) -> Result<MarkType> {
        let raw = match self {
            MarkDef::Shorthand(s) => s.as_str(),
            MarkDef::Full(p) => p.mark_type.as_str(),
        };
        MarkType::parse(raw)
    }

    pub fn props(&self) -> MarkProps {
        match self {
            MarkDef::Shorthand(s) => MarkProps {
                mark_type: s.clone(),
                ..Default::default()
            },
            MarkDef::Full(p) => p.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkType {
    Bar,
    Line,
    Point,
    /// Pie and donut slices.
    Arc,
}

impl MarkType {
    fn parse(raw: &str) -> Result<Self> {
        match raw {
            "bar" => Ok(MarkType::Bar),
            "line" => Ok(MarkType::Line),
            "point" | "circle" => Ok(MarkType::Point),
            "arc" => Ok(MarkType::Arc),
            other => Err(Error::UnsupportedMark(other.to_string())),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Encoding {
    #[serde(default)]
    pub x: Option<Channel>,
    #[serde(default)]
    pub y: Option<Channel>,
    #[serde(default)]
    pub color: Option<Channel>,
    /// Angular extent of an `arc` mark — the pie-chart equivalent of `y`.
    #[serde(default)]
    pub theta: Option<Channel>,
    #[serde(default)]
    pub radius: Option<Channel>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Channel {
    #[serde(default)]
    pub field: Option<String>,
    #[serde(rename = "type", default)]
    pub field_type: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub scale: Option<ScaleDef>,
}

impl Channel {
    pub fn field_name(&self, channel: &'static str) -> Result<&str> {
        self.field
            .as_deref()
            .ok_or(Error::MissingField(channel))
    }

    /// Resolves the declared type. Vega-Lite infers this when omitted; we only
    /// infer between quantitative and nominal, which is all the v0 surface needs.
    pub fn resolve_type(&self, inferred_numeric: bool) -> Result<FieldType> {
        match self.field_type.as_deref() {
            Some("quantitative") => Ok(FieldType::Quantitative),
            Some("nominal") => Ok(FieldType::Nominal),
            Some("ordinal") => Ok(FieldType::Ordinal),
            Some(other) => Err(Error::UnsupportedFieldType(other.to_string())),
            None if inferred_numeric => Ok(FieldType::Quantitative),
            None => Ok(FieldType::Nominal),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldType {
    Quantitative,
    Nominal,
    Ordinal,
}

impl FieldType {
    /// Nominal and ordinal both map onto a discrete (band/point) scale; only the
    /// sort order differs, and v0 sorts both by first appearance.
    pub fn is_discrete(self) -> bool {
        matches!(self, FieldType::Nominal | FieldType::Ordinal)
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScaleDef {
    #[serde(default)]
    pub zero: Option<bool>,
    #[serde(default)]
    pub nice: Option<bool>,
    #[serde(default)]
    pub domain: Option<Vec<serde_json::Value>>,
    #[serde(default)]
    pub padding_inner: Option<f32>,
    #[serde(default)]
    pub padding_outer: Option<f32>,
    #[serde(default)]
    pub range: Option<Vec<serde_json::Value>>,
}

impl Spec {
    pub fn parse(json: &str) -> Result<Self> {
        serde_json::from_str(json).map_err(Error::Parse)
    }

    /// Plot-area dimensions. Note these are the *inner* data rectangle, matching
    /// Vega-Lite semantics — the finished image is larger, to fit axes and padding.
    pub fn plot_size(&self) -> Result<(f32, f32)> {
        let w = self.width.unwrap_or(DEFAULT_WIDTH);
        let h = self.height.unwrap_or(DEFAULT_HEIGHT);
        for (name, v) in [("width", w), ("height", h)] {
            if !v.is_finite() || v <= 0.0 {
                return Err(Error::InvalidSize(format!("{name} must be positive")));
            }
            if v > MAX_DIMENSION {
                return Err(Error::InvalidSize(format!(
                    "{name} exceeds the {MAX_DIMENSION} limit"
                )));
            }
        }
        Ok((w, h))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_mark_shorthand_and_object() {
        let a: MarkDef = serde_json::from_str(r#""bar""#).unwrap();
        let b: MarkDef = serde_json::from_str(r##"{"type":"bar","color":"#f00"}"##).unwrap();
        assert_eq!(a.mark_type().unwrap(), MarkType::Bar);
        assert_eq!(b.mark_type().unwrap(), MarkType::Bar);
        assert_eq!(b.props().color.as_deref(), Some("#f00"));
    }

    #[test]
    fn ignores_unknown_fields_so_real_vega_lite_specs_parse() {
        let spec = Spec::parse(
            r#"{
                "$schema": "https://vega.github.io/schema/vega-lite/v5.json",
                "description": "ignored",
                "data": {"values": [{"a": "A", "b": 1}]},
                "mark": "bar",
                "encoding": {
                    "x": {"field": "a", "type": "nominal", "axis": {"labelAngle": 0}},
                    "y": {"field": "b", "type": "quantitative"}
                }
            }"#,
        );
        assert!(spec.is_ok(), "{:?}", spec.err());
    }

    #[test]
    fn rejects_unsupported_mark_rather_than_drawing_nothing() {
        let m = MarkDef::Shorthand("area".into());
        assert!(matches!(m.mark_type(), Err(Error::UnsupportedMark(_))));
    }

    #[test]
    fn rejects_temporal_field_type() {
        let c = Channel {
            field_type: Some("temporal".into()),
            ..Default::default()
        };
        assert!(matches!(
            c.resolve_type(false),
            Err(Error::UnsupportedFieldType(_))
        ));
    }

    #[test]
    fn rejects_remote_data() {
        let d = DataDef {
            values: None,
            url: Some("https://example.com/data.json".into()),
            name: None,
        };
        assert!(matches!(d.rows(), Err(Error::UnsupportedData(_))));
    }

    #[test]
    fn rejects_degenerate_sizes() {
        let base = r#"{"data":{"values":[]},"mark":"bar","encoding":{}"#;
        for bad in ["0", "-10", "1e9"] {
            let spec = Spec::parse(&format!("{base},\"width\":{bad}}}")).unwrap();
            assert!(
                matches!(spec.plot_size(), Err(Error::InvalidSize(_))),
                "width {bad} should be rejected"
            );
        }
    }
}
