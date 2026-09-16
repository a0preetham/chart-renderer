//! Categorical colour scheme.
//!
//! Vega's default categorical scheme is `tableau10`, and Vega-Lite's default
//! single-mark colour is its first entry — which is why an uncoloured bar and
//! the first series of a coloured one come out the same blue.

use crate::ir::Color;

/// Vega's `tableau10`, in order.
pub const TABLEAU10: [Color; 10] = [
    Color::rgb(0x4c, 0x78, 0xa8),
    Color::rgb(0xf5, 0x85, 0x18),
    Color::rgb(0xe4, 0x57, 0x56),
    Color::rgb(0x72, 0xb7, 0xb2),
    Color::rgb(0x54, 0xa2, 0x4b),
    Color::rgb(0xee, 0xca, 0x3b),
    Color::rgb(0xb2, 0x79, 0xa2),
    Color::rgb(0xff, 0x9d, 0xa6),
    Color::rgb(0x9d, 0x75, 0x5d),
    Color::rgb(0xba, 0xb0, 0xac),
];

/// Colour for the `index`-th category, wrapping once the scheme runs out —
/// which is what Vega does rather than failing or fading.
pub fn categorical(index: usize) -> Color {
    TABLEAU10[index % TABLEAU10.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::marks::DEFAULT_MARK_COLOR;

    #[test]
    fn the_first_entry_is_the_default_mark_colour() {
        assert_eq!(categorical(0), DEFAULT_MARK_COLOR);
    }

    #[test]
    fn distinct_categories_get_distinct_colours_until_the_scheme_wraps() {
        let first_ten: Vec<Color> = (0..10).map(categorical).collect();
        for (i, a) in first_ten.iter().enumerate() {
            for b in &first_ten[i + 1..] {
                assert_ne!(a, b, "scheme has a duplicate");
            }
        }
        assert_eq!(categorical(10), categorical(0), "should wrap");
    }
}
