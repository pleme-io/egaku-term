//! Terminal motion: time-parameterised helpers over the fleet's motion
//! algebra.
//!
//! The curves, durations and evaluators are `ishou_tokens::motion`'s (the
//! same ones mado's bell flash and tobira use); this module only adds what
//! a cell grid needs on top of them: spinner frames, a pulse, a shimmer
//! band, a reveal count and a frame pacer. Every function takes the time
//! as an argument, so a render is a pure function of `(state, now)` and a
//! test pins any instant without sleeping.

use std::time::{Duration, Instant};

pub use ishou_tokens::motion::{Advance, Curve, Durations, EasingKind, Motion, Tween};

/// The fleet's named durations (`fast`, `base`, …) as `Duration`s.
#[must_use]
pub fn duration(pick: fn(&Durations) -> u16) -> Duration {
    Duration::from_millis(u64::from(pick(&Motion::default().duration)))
}

/// Eased progress `0..=1` of an animation `duration` long, `elapsed` in.
/// A zero duration is already complete.
#[must_use]
pub fn progress(elapsed: Duration, duration: Duration, curve: Curve) -> f32 {
    if duration.is_zero() {
        return 1.0;
    }
    curve.ease(elapsed.as_secs_f32() / duration.as_secs_f32())
}

/// How many of `total` units (characters, rows) are shown `elapsed` into
/// a reveal lasting `duration`. Never zero once started with work to show,
/// and exactly `total` at the end.
#[must_use]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
pub fn reveal_count(total: usize, elapsed: Duration, duration: Duration, curve: Curve) -> usize {
    if total == 0 {
        return 0;
    }
    let p = progress(elapsed, duration, curve);
    ((total as f32 * p).ceil() as usize).clamp(1, total)
}

/// A 0..=1 cosine breath with the given period: 0 at `t = 0`, 1 at half
/// a period. For a pulsing dot or a border that breathes while busy.
#[must_use]
pub fn pulse(elapsed: Duration, period: Duration) -> f32 {
    if period.is_zero() {
        return 0.0;
    }
    let phase = (elapsed.as_secs_f32() / period.as_secs_f32()).fract();
    0.5 - 0.5 * (phase * std::f32::consts::TAU).cos()
}

/// Intensity 0..=1 of a highlight band sweeping left to right across
/// `width` cells once per `period`, at column `col`. The band is about a
/// fifth of the width, soft at its edges; it starts and ends off-screen so
/// the sweep enters and leaves rather than popping.
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn shimmer(col: usize, width: usize, elapsed: Duration, period: Duration) -> f32 {
    if width == 0 || period.is_zero() {
        return 0.0;
    }
    let w = width as f32;
    let half = (w / 10.0).max(1.5);
    let phase = (elapsed.as_secs_f32() / period.as_secs_f32()).fract();
    let centre = -half + phase * (w + 2.0 * half);
    let d = ((col as f32 + 0.5) - centre).abs() / half;
    if d >= 1.0 {
        0.0
    } else {
        0.5 + 0.5 * (d * std::f32::consts::PI).cos()
    }
}

/// A busy indicator's frame set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Spinner {
    /// Braille dots circling: `⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏`.
    #[default]
    Braille,
    /// A growing and shrinking dot: `·•●•`.
    Breath,
    /// ASCII only: `|/-\`, for terminals without the glyphs.
    Line,
}

impl Spinner {
    /// The frames, in order.
    #[must_use]
    pub const fn frames(self) -> &'static [&'static str] {
        match self {
            Self::Braille => &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"],
            Self::Breath => &["·", "•", "●", "•"],
            Self::Line => &["|", "/", "-", "\\"],
        }
    }

    /// Time per frame.
    #[must_use]
    pub const fn interval(self) -> Duration {
        match self {
            Self::Braille => Duration::from_millis(80),
            Self::Breath => Duration::from_millis(180),
            Self::Line => Duration::from_millis(120),
        }
    }

    /// The frame showing `elapsed` after the spinner started.
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub fn frame(self, elapsed: Duration) -> &'static str {
        let f = self.frames();
        let i = (elapsed.as_millis() / self.interval().as_millis().max(1)) as usize % f.len();
        f[i]
    }
}

/// Paces repaints: at most one frame per `interval`, and tells the event
/// loop how long it may block before the next frame is due. Explicit
/// `now` everywhere, so it is deterministic under test.
#[derive(Debug, Clone, Copy)]
pub struct FramePacer {
    interval: Duration,
    last: Option<Instant>,
}

impl FramePacer {
    /// A pacer targeting `fps` frames a second (clamped to 1..=240).
    #[must_use]
    pub fn new(fps: u32) -> Self {
        let fps = fps.clamp(1, 240);
        Self {
            interval: Duration::from_secs(1) / fps,
            last: None,
        }
    }

    /// The frame interval.
    #[must_use]
    pub const fn interval(&self) -> Duration {
        self.interval
    }

    /// Whether a frame may be drawn at `now`.
    #[must_use]
    pub fn due(&self, now: Instant) -> bool {
        self.last
            .is_none_or(|l| now.saturating_duration_since(l) >= self.interval)
    }

    /// Record that a frame was drawn at `now`.
    pub fn mark(&mut self, now: Instant) {
        self.last = Some(now);
    }

    /// How long to wait for input before the next frame is due, when
    /// something is animating; `idle` when nothing is.
    #[must_use]
    pub fn timeout(&self, now: Instant, animating: bool, idle: Duration) -> Duration {
        if !animating {
            return idle;
        }
        self.last.map_or(Duration::ZERO, |l| {
            self.interval
                .saturating_sub(now.saturating_duration_since(l))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: fn(u64) -> Duration = Duration::from_millis;

    #[test]
    fn progress_is_eased_and_bounded() {
        let c = Curve::named(EasingKind::Decelerate);
        assert!((progress(MS(0), MS(200), c)).abs() < 1e-6);
        assert!((progress(MS(200), MS(200), c) - 1.0).abs() < 1e-6);
        assert!((progress(MS(999), MS(200), c) - 1.0).abs() < 1e-6);
        assert!(
            progress(MS(100), MS(200), c) > 0.5,
            "decelerate front-loads"
        );
        assert!((progress(MS(5), Duration::ZERO, c) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn reveal_grows_monotonically_to_total() {
        let c = Curve::Linear;
        let counts: Vec<usize> = (0..=10)
            .map(|i| reveal_count(40, MS(i * 20), MS(200), c))
            .collect();
        assert_eq!(counts[0], 1);
        assert_eq!(counts[10], 40);
        assert!(counts.windows(2).all(|w| w[0] <= w[1]));
        assert_eq!(reveal_count(0, MS(50), MS(200), c), 0);
    }

    #[test]
    fn pulse_breathes() {
        let p = MS(1000);
        assert!(pulse(MS(0), p) < 1e-6);
        assert!((pulse(MS(500), p) - 1.0).abs() < 1e-6);
        assert!(pulse(MS(1000), p) < 1e-3);
        assert!(pulse(MS(1), Duration::ZERO).abs() < 1e-6);
    }

    #[test]
    fn shimmer_sweeps_left_to_right() {
        let per = MS(2000);
        let peak = |t: u64| {
            (0..40)
                .max_by(|a, b| shimmer(*a, 40, MS(t), per).total_cmp(&shimmer(*b, 40, MS(t), per)))
                .unwrap()
        };
        assert!(peak(500) < peak(1000) && peak(1000) < peak(1500));
        assert!((0..40).all(|c| (0.0..=1.0).contains(&shimmer(c, 40, MS(700), per))));
        assert!(
            (0..40).all(|c| shimmer(c, 40, MS(0), per) < 0.01),
            "the band starts off-screen"
        );
    }

    #[test]
    fn spinner_steps_per_interval_and_wraps() {
        let s = Spinner::Braille;
        assert_eq!(s.frame(MS(0)), "⠋");
        assert_eq!(s.frame(MS(80)), "⠙");
        assert_eq!(s.frame(MS(79)), "⠋");
        assert_eq!(s.frame(MS(800)), "⠋", "ten frames wrap");
        assert_eq!(Spinner::Line.frame(MS(360)), "\\");
    }

    #[test]
    fn pacer_caps_rate_and_sizes_the_wait() {
        let t0 = Instant::now();
        let mut p = FramePacer::new(50);
        assert_eq!(p.interval(), MS(20));
        assert!(p.due(t0));
        assert_eq!(p.timeout(t0, true, MS(250)), Duration::ZERO);
        p.mark(t0);
        assert!(!p.due(t0 + MS(5)));
        assert!(p.due(t0 + MS(20)));
        assert_eq!(p.timeout(t0 + MS(5), true, MS(250)), MS(15));
        assert_eq!(p.timeout(t0 + MS(5), false, MS(250)), MS(250));
        assert_eq!(p.timeout(t0 + MS(90), true, MS(250)), Duration::ZERO);
    }

    #[test]
    fn named_durations_come_from_the_tokens() {
        assert_eq!(duration(|d| d.fast_ms), MS(150));
    }
}
