//! The vector IR that mark generation emits.
//!
//! Everything downstream of `marks` consumes a [`Scene`] rather than
//! rasterizer-specific types. That keeps the scene inspectable by tests and
//! leaves room for a second (e.g. SVG) backend over the same data.
//!
//! Note that [`SceneItem::Text`] carries the string, not glyph outlines —
//! shaping happens in the render backend. Layout still needs text *measurement*,
//! which is why it takes a `TextShaper` and calls only `measure_width`.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub const WHITE: Self = Self::rgb(255, 255, 255);
    pub const BLACK: Self = Self::rgb(0, 0, 0);

    /// Parses `#rgb`, `#rrggbb`, or `#rrggbbaa`. Returns `None` for anything else;
    /// callers fall back to a default rather than failing the render.
    pub fn from_hex(s: &str) -> Option<Self> {
        let h = s.strip_prefix('#')?;
        let nib = |c: u8| -> Option<u8> {
            match c {
                b'0'..=b'9' => Some(c - b'0'),
                b'a'..=b'f' => Some(c - b'a' + 10),
                b'A'..=b'F' => Some(c - b'A' + 10),
                _ => None,
            }
        };
        let b = h.as_bytes();
        let byte = |i: usize| -> Option<u8> { Some(nib(b[i])? << 4 | nib(b[i + 1])?) };
        match b.len() {
            3 => {
                let d = |i: usize| nib(b[i]).map(|v| v << 4 | v);
                Some(Self::rgb(d(0)?, d(1)?, d(2)?))
            }
            6 => Some(Self::rgb(byte(0)?, byte(2)?, byte(4)?)),
            8 => Some(Self::rgba(byte(0)?, byte(2)?, byte(4)?, byte(6)?)),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Stroke {
    pub color: Color,
    pub width: f32,
}

impl Stroke {
    pub const fn new(color: Color, width: f32) -> Self {
        Self { color, width }
    }
}

/// Horizontal alignment of text relative to its anchor point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Anchor {
    Start,
    Middle,
    End,
}

/// Vertical alignment of text relative to its anchor point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Baseline {
    Top,
    Middle,
    Bottom,
    Alphabetic,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum SceneItem {
    Rect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        fill: Option<Color>,
        stroke: Option<Stroke>,
    },
    Line {
        points: Vec<(f32, f32)>,
        stroke: Stroke,
    },
    Circle {
        cx: f32,
        cy: f32,
        r: f32,
        fill: Option<Color>,
        stroke: Option<Stroke>,
    },
    /// A pie or donut slice.
    ///
    /// Angles follow Vega: **0 points up (12 o'clock) and increases clockwise**,
    /// in radians. `inner_radius` of 0 gives a pie; anything larger gives a donut.
    Arc {
        cx: f32,
        cy: f32,
        inner_radius: f32,
        outer_radius: f32,
        start_angle: f32,
        end_angle: f32,
        fill: Option<Color>,
        stroke: Option<Stroke>,
    },
    Text {
        x: f32,
        y: f32,
        content: String,
        size: f32,
        anchor: Anchor,
        baseline: Baseline,
        fill: Color,
        /// Clockwise rotation in degrees about `(x, y)`. Vega rotates the y-axis
        /// title by -90 so it runs up the axis; everything else is 0.
        angle: f32,
    },
}

/// The data rectangle, in canvas coordinates.
///
/// Recorded on the scene because consumers need it: the Vega cross-check
/// normalises against it, and the demo uses it to align panes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct PlotRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Scene {
    pub width: f32,
    pub height: f32,
    pub background: Color,
    pub plot: PlotRect,
    pub items: Vec<SceneItem>,
}

impl Scene {
    pub fn new(width: f32, height: f32, background: Color) -> Self {
        Self {
            width,
            height,
            background,
            plot: PlotRect { x: 0.0, y: 0.0, w: width, h: height },
            items: Vec::new(),
        }
    }

    pub fn with_plot(mut self, plot: PlotRect) -> Self {
        self.plot = plot;
        self
    }

    pub fn push(&mut self, item: SceneItem) {
        self.items.push(item);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_colors() {
        assert_eq!(Color::from_hex("#4c78a8"), Some(Color::rgb(0x4c, 0x78, 0xa8)));
        assert_eq!(Color::from_hex("#f0a"), Some(Color::rgb(0xff, 0x00, 0xaa)));
        assert_eq!(
            Color::from_hex("#4c78a880"),
            Some(Color::rgba(0x4c, 0x78, 0xa8, 0x80))
        );
    }

    #[test]
    fn rejects_malformed_hex_without_panicking() {
        for s in ["", "#", "4c78a8", "#xyzxyz", "#12345", "#1234567890"] {
            assert_eq!(Color::from_hex(s), None, "expected None for {s:?}");
        }
    }
}
