//! Data transforms — the stage between columns and scales.
//!
//! Everything before this mapped one data row directly onto one piece of
//! geometry. Stacking is the first thing that cannot: a pie slice's start angle
//! depends on every row that precedes it. The same machinery is what stacked
//! bars need, which is why this is a stage rather than something folded into the
//! arc mark.

use crate::data::NumericColumn;

/// A stacked row's cumulative span, `[start, end]`.
pub type Band = (f64, f64);

/// Result of stacking a column.
#[derive(Debug, Clone, PartialEq)]
pub struct Stacked {
    /// Cumulative band per row, aligned with the input column. `None` where the
    /// row had no value and contributes nothing.
    pub bands: Vec<Option<Band>>,
    /// Sum of the contributing values — the natural domain maximum.
    pub total: f64,
}

/// Accumulates `values` into cumulative bands, visiting rows in `order`.
///
/// **`order` is not row order, and that matters.** Vega stacks in the order of
/// the colour scale's *domain*, which is sorted — data rows `Zeta, Alpha, Mid`
/// produce slices in the order `Alpha, Mid, Zeta`. Stacking in row order instead
/// silently rearranges every chart whose data is not already sorted.
///
/// Rows absent from `order`, out of range, or holding no value get `None`.
/// Non-positive values contribute nothing: a negative slice has no sensible
/// meaning in a pie, and letting one through would wind the remaining slices
/// backwards.
pub fn stack(values: &NumericColumn, order: &[usize]) -> Stacked {
    let mut bands = vec![None; values.len()];
    let mut cursor = 0.0;

    for &row in order {
        let Some(Some(value)) = values.get(row).copied() else {
            continue;
        };
        if !value.is_finite() || value <= 0.0 {
            continue;
        }
        let next = cursor + value;
        bands[row] = Some((cursor, next));
        cursor = next;
    }

    Stacked {
        bands,
        total: cursor,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bands_are_contiguous_and_sum_to_the_total() {
        let values = vec![Some(4.0), Some(6.0), Some(10.0)];
        let s = stack(&values, &[0, 1, 2]);
        assert_eq!(s.total, 20.0);
        assert_eq!(s.bands[0], Some((0.0, 4.0)));
        assert_eq!(s.bands[1], Some((4.0, 10.0)));
        assert_eq!(s.bands[2], Some((10.0, 20.0)));
    }

    #[test]
    fn order_drives_accumulation_not_row_position() {
        // Rows given in the order 1, 2, 0 must accumulate in that order while
        // the results stay indexed by row — this is what makes a chart's slices
        // follow the sorted colour domain rather than the data's row order.
        let values = vec![Some(10.0), Some(20.0), Some(5.0)];
        let s = stack(&values, &[1, 2, 0]);
        assert_eq!(s.bands[1], Some((0.0, 20.0)));
        assert_eq!(s.bands[2], Some((20.0, 25.0)));
        assert_eq!(s.bands[0], Some((25.0, 35.0)));
        assert_eq!(s.total, 35.0);
    }

    #[test]
    fn missing_and_non_positive_values_contribute_nothing() {
        let values = vec![Some(5.0), None, Some(-3.0), Some(0.0), Some(5.0)];
        let s = stack(&values, &[0, 1, 2, 3, 4]);
        assert_eq!(s.total, 10.0);
        assert_eq!(s.bands[0], Some((0.0, 5.0)));
        assert_eq!(s.bands[1], None);
        assert_eq!(s.bands[2], None, "a negative slice must not wind backwards");
        assert_eq!(s.bands[3], None);
        assert_eq!(s.bands[4], Some((5.0, 10.0)));
    }

    #[test]
    fn rows_outside_the_order_are_left_out() {
        let values = vec![Some(1.0), Some(2.0), Some(3.0)];
        let s = stack(&values, &[0, 2]);
        assert_eq!(s.bands[1], None);
        assert_eq!(s.total, 4.0);
    }

    #[test]
    fn an_out_of_range_index_is_ignored_rather_than_panicking() {
        let values = vec![Some(1.0)];
        let s = stack(&values, &[0, 99]);
        assert_eq!(s.total, 1.0);
        assert_eq!(s.bands.len(), 1);
    }

    #[test]
    fn an_empty_column_totals_zero() {
        let s = stack(&Vec::new(), &[]);
        assert_eq!(s.total, 0.0);
        assert!(s.bands.is_empty());
    }
}
