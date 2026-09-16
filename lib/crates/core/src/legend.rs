//! Categorical colour legend.
//!
//! Sized before layout, like the axes: the legend's width becomes part of the
//! right inset, so the plot keeps its requested size and the canvas grows.

use crate::ir::{Anchor, Baseline, Color, Scene, SceneItem, Stroke};
use crate::layout::Rect;
use crate::spec::MarkType;
use crate::text::TextShaper;

#[derive(Debug, Clone, Copy)]
pub struct LegendStyle {
    pub label_font_size: f32,
    pub title_font_size: f32,
    /// Symbol area in px², as Vega measures symbol size.
    pub symbol_size: f32,
    pub symbol_gap: f32,
    pub row_padding: f32,
    /// Gap between the plot's right edge and the legend.
    pub offset: f32,
    pub title_padding: f32,
    pub label_color: Color,
    pub title_color: Color,
}

impl Default for LegendStyle {
    fn default() -> Self {
        Self {
            label_font_size: 10.0,
            title_font_size: 11.0,
            symbol_size: 100.0,
            symbol_gap: 7.0,
            row_padding: 2.0,
            offset: 18.0,
            title_padding: 5.0,
            label_color: Color::BLACK,
            title_color: Color::BLACK,
        }
    }
}

impl LegendStyle {
    /// Vega expresses symbol size as an area; drawing needs a radius.
    pub fn symbol_radius(&self) -> f32 {
        (self.symbol_size / std::f32::consts::PI).sqrt()
    }

    pub fn row_height(&self, shaper: &dyn TextShaper) -> f32 {
        let text = shaper.ascent(self.label_font_size) + shaper.descent(self.label_font_size);
        text.max(self.symbol_radius() * 2.0) + self.row_padding
    }
}

#[derive(Debug, Clone)]
pub struct Legend {
    pub title: Option<String>,
    pub entries: Vec<(String, Color)>,
    /// Drives the symbol shape, so the legend matches what is plotted.
    pub mark: MarkType,
}

/// Horizontal space the legend needs, including its offset from the plot.
pub fn width(legend: &Legend, shaper: &dyn TextShaper, style: &LegendStyle) -> f32 {
    if legend.entries.is_empty() {
        return 0.0;
    }
    let widest_label = legend
        .entries
        .iter()
        .map(|(label, _)| shaper.measure_width(label, style.label_font_size))
        .fold(0.0f32, f32::max);
    let body = style.symbol_radius() * 2.0 + style.symbol_gap + widest_label;
    let title = legend
        .title
        .as_deref()
        .map(|t| shaper.measure_width(t, style.title_font_size))
        .unwrap_or(0.0);
    style.offset + body.max(title)
}

/// Vertical space the legend needs, so the caller can check it against the plot.
pub fn height(legend: &Legend, shaper: &dyn TextShaper, style: &LegendStyle) -> f32 {
    if legend.entries.is_empty() {
        return 0.0;
    }
    let rows = legend.entries.len() as f32 * style.row_height(shaper);
    let title = if legend.title.is_some() {
        style.title_font_size + style.title_padding
    } else {
        0.0
    };
    rows + title
}

pub fn emit(
    scene: &mut Scene,
    plot: &Rect,
    legend: &Legend,
    style: &LegendStyle,
    shaper: &dyn TextShaper,
) {
    if legend.entries.is_empty() {
        return;
    }
    let x = plot.right() + style.offset;
    let mut y = plot.y;

    if let Some(title) = &legend.title {
        scene.push(SceneItem::Text {
            x,
            y,
            content: title.clone(),
            size: style.title_font_size,
            anchor: Anchor::Start,
            baseline: Baseline::Top,
            fill: style.title_color,
            angle: 0.0,
        });
        y += style.title_font_size + style.title_padding;
    }

    let radius = style.symbol_radius();
    let row = style.row_height(shaper);

    for (label, color) in &legend.entries {
        let centre_y = y + row / 2.0;
        let centre_x = x + radius;

        match legend.mark {
            // A point legend mirrors the mark: stroked, unfilled.
            MarkType::Point => scene.push(SceneItem::Circle {
                cx: centre_x,
                cy: centre_y,
                r: radius,
                fill: None,
                stroke: Some(Stroke::new(*color, 2.0)),
            }),
            MarkType::Line => scene.push(SceneItem::Line {
                points: vec![
                    (centre_x - radius, centre_y),
                    (centre_x + radius, centre_y),
                ],
                stroke: Stroke::new(*color, 2.0),
            }),
            // A pie slice and a bar both read as a filled swatch.
            MarkType::Bar | MarkType::Arc => scene.push(SceneItem::Rect {
                x: centre_x - radius,
                y: centre_y - radius,
                w: radius * 2.0,
                h: radius * 2.0,
                fill: Some(*color),
                stroke: None,
            }),
        }

        scene.push(SceneItem::Text {
            x: centre_x + radius + style.symbol_gap,
            y: centre_y,
            content: label.clone(),
            size: style.label_font_size,
            anchor: Anchor::Start,
            baseline: Baseline::Middle,
            fill: style.label_color,
            angle: 0.0,
        });

        y += row;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::default_shaper;

    fn legend(entries: usize) -> Legend {
        Legend {
            title: Some("series".into()),
            entries: (0..entries)
                .map(|i| (format!("series {i}"), crate::palette::categorical(i)))
                .collect(),
            mark: MarkType::Bar,
        }
    }

    #[test]
    fn an_empty_legend_takes_no_space_and_draws_nothing() {
        let shaper = default_shaper();
        let style = LegendStyle::default();
        let empty = Legend {
            title: None,
            entries: Vec::new(),
            mark: MarkType::Bar,
        };
        assert_eq!(width(&empty, &shaper, &style), 0.0);
        assert_eq!(height(&empty, &shaper, &style), 0.0);

        let mut scene = Scene::new(10.0, 10.0, Color::WHITE);
        emit(
            &mut scene,
            &Rect {
                x: 0.0,
                y: 0.0,
                w: 10.0,
                h: 10.0,
            },
            &empty,
            &style,
            &shaper,
        );
        assert!(scene.items.is_empty());
    }

    #[test]
    fn width_grows_with_the_longest_label() {
        let shaper = default_shaper();
        let style = LegendStyle::default();
        let mut long = legend(2);
        long.entries[0].0 = "a very much longer series name".into();
        assert!(width(&long, &shaper, &style) > width(&legend(2), &shaper, &style));
    }

    #[test]
    fn height_grows_one_row_at_a_time() {
        let shaper = default_shaper();
        let style = LegendStyle::default();
        let one = height(&legend(1), &shaper, &style);
        let three = height(&legend(3), &shaper, &style);
        assert!((three - one - 2.0 * style.row_height(&shaper)).abs() < 1e-3);
    }

    #[test]
    fn emits_a_symbol_and_a_label_per_entry() {
        let shaper = default_shaper();
        let style = LegendStyle::default();
        let mut scene = Scene::new(300.0, 200.0, Color::WHITE);
        let plot = Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        };
        emit(&mut scene, &plot, &legend(3), &style, &shaper);

        let texts = scene
            .items
            .iter()
            .filter(|i| matches!(i, SceneItem::Text { .. }))
            .count();
        let symbols = scene
            .items
            .iter()
            .filter(|i| matches!(i, SceneItem::Rect { .. }))
            .count();
        assert_eq!(symbols, 3);
        assert_eq!(texts, 4, "three labels plus the title");
    }

    #[test]
    fn entries_stack_downward_without_overlapping() {
        let shaper = default_shaper();
        let style = LegendStyle::default();
        let mut scene = Scene::new(300.0, 200.0, Color::WHITE);
        let plot = Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        };
        emit(&mut scene, &plot, &legend(3), &style, &shaper);

        let ys: Vec<f32> = scene
            .items
            .iter()
            .filter_map(|i| match i {
                SceneItem::Rect { y, .. } => Some(*y),
                _ => None,
            })
            .collect();
        for pair in ys.windows(2) {
            assert!(pair[1] > pair[0], "rows should descend: {ys:?}");
        }
    }
}
