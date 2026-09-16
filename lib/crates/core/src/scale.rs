//! Linear, band, and point scales.
//!
//! These follow d3-scale's algorithms, because that is what Vega — and
//! therefore Vega-Lite — uses. Reimplementing them approximately would put every
//! mark in a slightly wrong place, so the tick and band arithmetic below is
//! deliberately a faithful port rather than something simpler.

/// Vega-Lite's default inner padding for a band scale used by bar marks.
pub const DEFAULT_BAND_PADDING_INNER: f32 = 0.1;
/// Vega-Lite derives outer padding from inner padding for bar marks.
pub const DEFAULT_BAND_PADDING_OUTER: f32 = 0.05;
/// d3-scale's default outer padding for a point scale.
pub const DEFAULT_POINT_PADDING_OUTER: f32 = 0.5;

/// Tick count `nice()` rounds against.
///
/// This is **not** the axis tick count. Vega calls `nice` with d3's default of
/// 10 regardless of how many ticks the axis will actually show, then derives the
/// ticks from the nice'd domain using the size-based count. Using the axis count
/// in both places gives visibly different domains — it put `negative_bar` on
/// (-30, 20) where Vega has (-25, 15).
///
/// Note that the count alone does not reproduce Vega: `nice` also iterates. See
/// [`LinearScale::nice`].
pub const NICE_COUNT: usize = 10;

/// A continuous linear scale mapping `domain` onto `range`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LinearScale {
    domain: (f64, f64),
    range: (f32, f32),
}

impl LinearScale {
    pub fn new(domain: (f64, f64), range: (f32, f32)) -> Self {
        Self { domain, range }
    }

    pub fn domain(&self) -> (f64, f64) {
        self.domain
    }

    pub fn range(&self) -> (f32, f32) {
        self.range
    }

    /// Extends the domain outward to round tick boundaries, as d3's `nice()` does.
    ///
    /// **This iterates, and that is not incidental.** Widening the domain changes
    /// its span, which can change the tick increment, which can widen it further.
    /// d3 repeats until the increment stabilises. A single pass lands one step
    /// short: for (0, 31) the sequence is 31 -> 32 -> 35, and stopping at 32 puts
    /// every gridline in the wrong place relative to Vega.
    pub fn nice(mut self, count: usize) -> Self {
        let (mut start, mut stop) = self.domain;
        if !start.is_finite() || !stop.is_finite() {
            return self;
        }
        let flipped = stop < start;
        if flipped {
            std::mem::swap(&mut start, &mut stop);
        }

        let mut prestep = f64::NAN;
        // d3 caps the loop at 10 rounds; in practice it converges in two or three.
        for _ in 0..10 {
            let step = tick_increment(start, stop, count.max(1));
            if step == prestep {
                break;
            }
            if step > 0.0 {
                start = (start / step).floor() * step;
                stop = (stop / step).ceil() * step;
            } else if step < 0.0 {
                // A negative increment encodes a reciprocal; see `tick_increment`.
                start = (start * step).ceil() / step;
                stop = (stop * step).floor() / step;
            } else {
                break;
            }
            prestep = step;
        }

        self.domain = if flipped { (stop, start) } else { (start, stop) };
        self
    }

    /// Maps a domain value into the range.
    ///
    /// A zero-width domain (every datum equal, or a single datum) would divide by
    /// zero, so it collapses to the range midpoint — which is where Vega places a
    /// constant series too.
    pub fn scale(&self, value: f64) -> f32 {
        let (d0, d1) = self.domain;
        let (r0, r1) = self.range;
        let span = d1 - d0;
        if span == 0.0 || !span.is_finite() {
            return (r0 + r1) / 2.0;
        }
        let t = (value - d0) / span;
        r0 + (t as f32) * (r1 - r0)
    }

    /// Approximately `count` round tick values spanning the domain.
    pub fn ticks(&self, count: usize) -> Vec<f64> {
        ticks(self.domain.0, self.domain.1, count.max(1))
    }
}

/// A discrete scale assigning each domain entry an evenly spaced band.
#[derive(Debug, Clone, PartialEq)]
pub struct BandScale {
    domain: Vec<String>,
    range: (f32, f32),
    padding_inner: f32,
    padding_outer: f32,
    align: f32,
    /// Cached because `scale()` is called once per datum.
    step: f32,
    bandwidth: f32,
    start: f32,
}

impl BandScale {
    pub fn new(domain: Vec<String>, range: (f32, f32)) -> Self {
        Self::with_padding(
            domain,
            range,
            DEFAULT_BAND_PADDING_INNER,
            DEFAULT_BAND_PADDING_OUTER,
        )
    }

    pub fn with_padding(
        domain: Vec<String>,
        range: (f32, f32),
        padding_inner: f32,
        padding_outer: f32,
    ) -> Self {
        let mut s = Self {
            domain,
            range,
            padding_inner: padding_inner.clamp(0.0, 1.0),
            padding_outer: padding_outer.max(0.0),
            align: 0.5,
            step: 0.0,
            bandwidth: 0.0,
            start: range.0,
        };
        s.rescale();
        s
    }

    /// A point scale is a band scale with no band width — d3 implements it
    /// exactly this way, as `paddingInner = 1`.
    pub fn point(domain: Vec<String>, range: (f32, f32)) -> Self {
        Self::with_padding(domain, range, 1.0, DEFAULT_POINT_PADDING_OUTER)
    }

    fn rescale(&mut self) {
        let n = self.domain.len() as f32;
        let (r0, r1) = self.range;
        let span = r1 - r0;
        if n == 0.0 {
            self.step = 0.0;
            self.bandwidth = 0.0;
            self.start = r0;
            return;
        }
        let divisor = n - self.padding_inner + self.padding_outer * 2.0;
        self.step = if divisor > 0.0 { span / divisor } else { 0.0 };
        self.bandwidth = self.step * (1.0 - self.padding_inner);
        self.start = r0 + (span - self.step * (n - self.padding_inner)) * self.align;
    }

    pub fn domain(&self) -> &[String] {
        &self.domain
    }

    pub fn range(&self) -> (f32, f32) {
        self.range
    }

    pub fn step(&self) -> f32 {
        self.step
    }

    pub fn bandwidth(&self) -> f32 {
        self.bandwidth
    }

    /// Start coordinate of the band for `value`, or `None` if it is not in the domain.
    pub fn scale(&self, value: &str) -> Option<f32> {
        let index = self.domain.iter().position(|d| d == value)?;
        Some(self.start + self.step * index as f32)
    }

    /// Centre of the band — where a point or line vertex sits.
    pub fn center(&self, value: &str) -> Option<f32> {
        self.scale(value).map(|x| x + self.bandwidth / 2.0)
    }
}

/// d3's `tickIncrement`. Assumes `start <= stop`.
///
/// The sign convention is d3's and is load-bearing: when the ideal step is below
/// 1, the result is the **negative reciprocal** (`-4` meaning a step of `1/4`)
/// rather than the fraction itself. Dividing by an exact integer avoids the
/// floating-point drift that repeatedly adding `0.25` would accumulate, so ticks
/// land on clean values.
fn tick_increment(start: f64, stop: f64, count: usize) -> f64 {
    let step = (stop - start) / count.max(1) as f64;
    if step <= 0.0 || !step.is_finite() {
        return 0.0;
    }
    let power = step.log10().floor();
    let error = step / 10f64.powf(power);
    // d3's thresholds, written as the square roots they are rather than as
    // opaque decimal literals.
    let factor = if error >= 50f64.sqrt() {
        10.0
    } else if error >= 10f64.sqrt() {
        5.0
    } else if error >= std::f64::consts::SQRT_2 {
        2.0
    } else {
        1.0
    };
    if power >= 0.0 {
        factor * 10f64.powf(power)
    } else {
        -(10f64.powf(-power)) / factor
    }
}

/// d3's `ticks`: round values at a round interval, covering as much of
/// `[start, stop]` as fits.
pub fn ticks(start: f64, stop: f64, count: usize) -> Vec<f64> {
    if !start.is_finite() || !stop.is_finite() {
        return Vec::new();
    }
    if start == stop {
        return vec![start];
    }
    let (lo, hi, reverse) = if start <= stop {
        (start, stop, false)
    } else {
        (stop, start, true)
    };

    let step = tick_increment(lo, hi, count.max(1));
    if step == 0.0 || !step.is_finite() {
        return Vec::new();
    }

    // d3 rounds and then nudges, rather than using ceil/floor directly: it keeps
    // a tick that lands within floating-point noise of the domain edge.
    let (r0, r1, divide) = if step > 0.0 {
        let mut r0 = (lo / step).round();
        let mut r1 = (hi / step).round();
        if r0 * step < lo {
            r0 += 1.0;
        }
        if r1 * step > hi {
            r1 -= 1.0;
        }
        (r0, r1, false)
    } else {
        let inverse = -step;
        let mut r0 = (lo * inverse).round();
        let mut r1 = (hi * inverse).round();
        if r0 / inverse < lo {
            r0 += 1.0;
        }
        if r1 / inverse > hi {
            r1 -= 1.0;
        }
        (r0, r1, true)
    };
    if !r0.is_finite() || !r1.is_finite() || r1 < r0 {
        return Vec::new();
    }
    // Guard against a pathological domain asking for millions of ticks.
    let n = (r1 - r0) as usize;
    if n > 10_000 {
        return Vec::new();
    }
    let mut out: Vec<f64> = if divide {
        let inverse = -step;
        (0..=n).map(|i| (r0 + i as f64) / inverse).collect()
    } else {
        (0..=n).map(|i| (r0 + i as f64) * step).collect()
    };
    if reverse {
        out.reverse();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn linear_maps_domain_onto_range() {
        let s = LinearScale::new((0.0, 100.0), (0.0, 200.0));
        assert!(close(s.scale(0.0), 0.0));
        assert!(close(s.scale(50.0), 100.0));
        assert!(close(s.scale(100.0), 200.0));
    }

    #[test]
    fn linear_handles_an_inverted_range_as_screen_y_requires() {
        // Screen y grows downward, so a y scale's range is (height, 0).
        let s = LinearScale::new((0.0, 10.0), (100.0, 0.0));
        assert!(close(s.scale(0.0), 100.0));
        assert!(close(s.scale(10.0), 0.0));
    }

    #[test]
    fn linear_collapses_a_zero_width_domain_to_the_range_midpoint() {
        let s = LinearScale::new((5.0, 5.0), (0.0, 200.0));
        assert!(close(s.scale(5.0), 100.0));
        assert!(close(s.scale(9.0), 100.0));
    }

    #[test]
    fn nice_rounds_the_domain_outward() {
        let s = LinearScale::new((0.3, 9.7), (0.0, 1.0)).nice(10);
        assert_eq!(s.domain(), (0.0, 10.0));

        let s = LinearScale::new((1.1, 10.9), (0.0, 1.0)).nice(5);
        assert_eq!(s.domain(), (0.0, 12.0));
    }

    #[test]
    fn ticks_match_d3() {
        assert_eq!(ticks(0.0, 1.0, 10).len(), 11);
        assert_eq!(ticks(0.0, 10.0, 5), vec![0.0, 2.0, 4.0, 6.0, 8.0, 10.0]);
        assert_eq!(ticks(0.0, 100.0, 5), vec![0.0, 20.0, 40.0, 60.0, 80.0, 100.0]);
        assert_eq!(ticks(1.0, 9.0, 4), vec![2.0, 4.0, 6.0, 8.0]);
    }

    #[test]
    fn ticks_of_a_degenerate_domain_do_not_hang_or_panic() {
        assert_eq!(ticks(5.0, 5.0, 10), vec![5.0]);
        assert!(ticks(f64::NAN, 1.0, 10).is_empty());
        assert!(ticks(0.0, f64::INFINITY, 10).is_empty());
        // A huge but finite domain is legitimate, and still yields ~count ticks.
        assert_eq!(ticks(0.0, 1e300, 10).len(), 11);
    }

    #[test]
    fn ticks_descend_for_a_reversed_domain() {
        assert_eq!(ticks(10.0, 0.0, 5), vec![10.0, 8.0, 6.0, 4.0, 2.0, 0.0]);
    }

    #[test]
    fn band_partitions_the_range() {
        let d = vec!["A".to_string(), "B".to_string(), "C".to_string()];
        let s = BandScale::with_padding(d, (0.0, 300.0), 0.0, 0.0);
        assert!(close(s.step(), 100.0));
        assert!(close(s.bandwidth(), 100.0));
        assert!(close(s.scale("A").unwrap(), 0.0));
        assert!(close(s.scale("B").unwrap(), 100.0));
        assert!(close(s.scale("C").unwrap(), 200.0));
        assert!(close(s.center("A").unwrap(), 50.0));
    }

    #[test]
    fn band_padding_matches_d3() {
        // d3: step = span / (n - paddingInner + 2 * paddingOuter)
        let d = vec!["A".to_string(), "B".to_string()];
        let s = BandScale::with_padding(d, (0.0, 100.0), 0.2, 0.1);
        let expected_step = 100.0 / (2.0 - 0.2 + 0.2);
        assert!(close(s.step(), expected_step));
        assert!(close(s.bandwidth(), expected_step * 0.8));
    }

    #[test]
    fn band_bands_stay_inside_the_range() {
        let d: Vec<String> = (0..7).map(|i| i.to_string()).collect();
        let s = BandScale::new(d.clone(), (10.0, 210.0));
        for v in &d {
            let x = s.scale(v).unwrap();
            assert!(x >= 10.0 - 1e-3, "{v}: {x} below range");
            assert!(x + s.bandwidth() <= 210.0 + 1e-3, "{v}: {x} above range");
        }
    }

    #[test]
    fn point_scale_places_first_and_last_symmetrically() {
        let d = vec!["A".to_string(), "B".to_string(), "C".to_string()];
        let s = BandScale::point(d, (0.0, 100.0));
        assert!(close(s.bandwidth(), 0.0));
        // step = 100 / (3 - 1 + 2*0.5) = 33.333
        assert!(close(s.step(), 100.0 / 3.0));
        let a = s.scale("A").unwrap();
        let c = s.scale("C").unwrap();
        assert!(close(a - 0.0, 100.0 - c), "padding should be symmetric");
    }

    #[test]
    fn empty_domain_does_not_divide_by_zero() {
        let s = BandScale::new(Vec::new(), (0.0, 100.0));
        assert_eq!(s.step(), 0.0);
        assert_eq!(s.bandwidth(), 0.0);
        assert_eq!(s.scale("A"), None);
    }

    #[test]
    fn single_entry_band_fills_the_range() {
        let s = BandScale::with_padding(vec!["only".into()], (0.0, 100.0), 0.0, 0.0);
        assert!(close(s.scale("only").unwrap(), 0.0));
        assert!(close(s.bandwidth(), 100.0));
    }
}
