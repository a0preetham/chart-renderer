//! Plot-area computation.
//!
//! Follows Vega-Lite's convention that `width`/`height` describe the **inner**
//! data rectangle, not the finished image. The canvas is that rectangle plus
//! whatever space the axes need plus outer padding, so a spec that says
//! `width: 300` gets a 300px-wide plot regardless of how long its y tick labels
//! turn out to be.

/// Vega-Lite's default outer padding.
pub const DEFAULT_PADDING: f32 = 5.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn contains(&self, x: f32, y: f32, tolerance: f32) -> bool {
        x >= self.x - tolerance
            && x <= self.right() + tolerance
            && y >= self.y - tolerance
            && y <= self.bottom() + tolerance
    }
}

/// Space reserved outside the plot rectangle, typically for axes.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Insets {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl Insets {
    pub const ZERO: Self = Self {
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
        left: 0.0,
    };
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    pub canvas_width: f32,
    pub canvas_height: f32,
    pub plot: Rect,
}

/// Places a `plot_w` x `plot_h` data rectangle inside a canvas big enough to
/// hold it, the given axis `insets`, and `padding` on every side.
pub fn compute(plot_w: f32, plot_h: f32, padding: f32, insets: Insets) -> Layout {
    let left = padding + insets.left;
    let top = padding + insets.top;
    Layout {
        canvas_width: left + plot_w + insets.right + padding,
        canvas_height: top + plot_h + insets.bottom + padding,
        plot: Rect {
            x: left,
            y: top,
            w: plot_w,
            h: plot_h,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canvas_grows_around_the_requested_plot_size() {
        let l = compute(300.0, 200.0, 5.0, Insets::ZERO);
        assert_eq!(l.plot.w, 300.0);
        assert_eq!(l.plot.h, 200.0);
        assert_eq!(l.canvas_width, 310.0);
        assert_eq!(l.canvas_height, 210.0);
        assert_eq!(l.plot.x, 5.0);
        assert_eq!(l.plot.y, 5.0);
    }

    #[test]
    fn axis_insets_do_not_shrink_the_plot() {
        let insets = Insets {
            top: 0.0,
            right: 0.0,
            bottom: 30.0,
            left: 40.0,
        };
        let l = compute(300.0, 200.0, 5.0, insets);
        // The plot keeps its requested size; the canvas absorbs the axis space.
        assert_eq!(l.plot.w, 300.0);
        assert_eq!(l.plot.h, 200.0);
        assert_eq!(l.plot.x, 45.0);
        assert_eq!(l.canvas_width, 350.0);
        assert_eq!(l.canvas_height, 240.0);
    }
}
