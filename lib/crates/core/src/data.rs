//! Turning inline `data.values` rows into the columns the scales need.
//!
//! Vega-Lite rows are heterogeneous JSON objects. Scales want either a numeric
//! column or a discrete one, so each encoded field is extracted once, up front,
//! and missing/non-conforming cells become `None` rather than errors — a null in
//! one row should drop that datum, not fail the chart.

use crate::error::{Error, Result};

/// A numeric column, aligned 1:1 with the input rows.
pub type NumericColumn = Vec<Option<f64>>;

/// A discrete column, aligned 1:1 with the input rows.
pub type DiscreteColumn = Vec<Option<String>>;

/// True if the field is present and numeric in at least one row, and never a
/// non-numeric non-null. Used only when the spec omits an explicit `type`.
pub fn looks_numeric(rows: &[serde_json::Value], field: &str) -> bool {
    let mut saw_number = false;
    for row in rows {
        match row.get(field) {
            Some(serde_json::Value::Number(_)) => saw_number = true,
            Some(serde_json::Value::Null) | None => {}
            Some(_) => return false,
        }
    }
    saw_number
}

/// Confirms the field appears in at least one row, so that a typo in the spec is
/// reported instead of silently producing an empty chart.
fn require_present(rows: &[serde_json::Value], field: &str) -> Result<()> {
    if rows.is_empty() || rows.iter().any(|r| r.get(field).is_some()) {
        Ok(())
    } else {
        Err(Error::UnknownField(field.to_string()))
    }
}

pub fn numeric_column(rows: &[serde_json::Value], field: &str) -> Result<NumericColumn> {
    require_present(rows, field)?;
    Ok(rows
        .iter()
        .map(|row| match row.get(field) {
            Some(serde_json::Value::Number(n)) => n.as_f64().filter(|v| v.is_finite()),
            Some(serde_json::Value::String(s)) => s.parse::<f64>().ok().filter(|v| v.is_finite()),
            Some(serde_json::Value::Bool(b)) => Some(if *b { 1.0 } else { 0.0 }),
            _ => None,
        })
        .collect())
}

pub fn discrete_column(rows: &[serde_json::Value], field: &str) -> Result<DiscreteColumn> {
    require_present(rows, field)?;
    Ok(rows
        .iter()
        .map(|row| match row.get(field) {
            Some(serde_json::Value::String(s)) => Some(s.clone()),
            Some(serde_json::Value::Number(n)) => Some(n.to_string()),
            Some(serde_json::Value::Bool(b)) => Some(b.to_string()),
            _ => None,
        })
        .collect())
}

/// Distinct values, **sorted**, which is Vega-Lite's default domain ordering for
/// a nominal or ordinal field with no explicit `sort`.
///
/// First-appearance order is the tempting implementation and it is wrong: given
/// rows `Zeta, Alpha, Mid`, Vega renders the bands `Alpha, Mid, Zeta`.
///
/// Rust orders `String` by UTF-8 bytes where JavaScript orders by UTF-16 code
/// units. Those agree across the BMP except for surrogate-range edge cases, so
/// this matches Vega for any realistic category label.
pub fn distinct(column: &DiscreteColumn) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for value in column.iter().flatten() {
        if !out.iter().any(|existing| existing == value) {
            out.push(value.clone());
        }
    }
    out.sort();
    out
}

/// Min and max over the defined cells. `None` when the column is entirely empty.
pub fn extent(column: &NumericColumn) -> Option<(f64, f64)> {
    let mut iter = column.iter().flatten().copied();
    let first = iter.next()?;
    Some(iter.fold((first, first), |(lo, hi), v| (lo.min(v), hi.max(v))))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(json: &str) -> Vec<serde_json::Value> {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn extracts_numeric_column_and_drops_nulls() {
        let r = rows(r#"[{"b":1},{"b":null},{"b":3},{}]"#);
        let col = numeric_column(&r, "b").unwrap();
        assert_eq!(col, vec![Some(1.0), None, Some(3.0), None]);
        assert_eq!(extent(&col), Some((1.0, 3.0)));
    }

    #[test]
    fn drops_non_finite_numbers() {
        // serde_json cannot hold NaN, but a string cell can parse to one.
        let r = rows(r#"[{"b":"NaN"},{"b":"inf"},{"b":"2"}]"#);
        assert_eq!(numeric_column(&r, "b").unwrap(), vec![None, None, Some(2.0)]);
    }

    #[test]
    fn distinct_sorts_like_vega_rather_than_keeping_input_order() {
        let r = rows(r#"[{"a":"Zeta"},{"a":"Alpha"},{"a":"Zeta"},{"a":null},{"a":"Mid"}]"#);
        let col = discrete_column(&r, "a").unwrap();
        assert_eq!(distinct(&col), vec!["Alpha", "Mid", "Zeta"]);
    }

    #[test]
    fn unknown_field_is_an_error_not_an_empty_chart() {
        let r = rows(r#"[{"a":1}]"#);
        assert!(matches!(
            numeric_column(&r, "typo"),
            Err(Error::UnknownField(_))
        ));
    }

    #[test]
    fn empty_data_does_not_trip_the_unknown_field_check() {
        let r: Vec<serde_json::Value> = vec![];
        assert_eq!(numeric_column(&r, "anything").unwrap(), Vec::new());
        assert_eq!(extent(&numeric_column(&r, "anything").unwrap()), None);
    }

    #[test]
    fn infers_numeric_only_when_every_present_cell_is_a_number() {
        let numeric = rows(r#"[{"b":1},{"b":null},{"b":3}]"#);
        let mixed = rows(r#"[{"b":1},{"b":"x"}]"#);
        let absent = rows(r#"[{"a":1}]"#);
        assert!(looks_numeric(&numeric, "b"));
        assert!(!looks_numeric(&mixed, "b"));
        assert!(!looks_numeric(&absent, "b"));
    }
}
