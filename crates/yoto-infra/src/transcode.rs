//! Waiting for Yoto to convert an uploaded track.
//!
//! Yoto converts every upload before it can be used in a playlist. A long track takes
//! minutes, so the time allowed grows with the track, a conversion whose progress stops
//! moving is noticed early, and a conversion Yoto reports as failed ends the wait at once.
use std::time::Duration;

/// Progress Yoto reports while it converts an upload.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Progress {
    pub phase: Option<String>,
    pub percent: Option<u8>,
}
impl Progress {
    /// Read `progress.phase` and `progress.percent` from a transcode envelope.
    pub fn read(transcode: &serde_json::Value) -> Self {
        let progress = transcode.get("progress");
        let phase = progress
            .and_then(|p| p.get("phase"))
            .and_then(serde_json::Value::as_str)
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty());
        let percent = progress
            .and_then(|p| p.get("percent"))
            .and_then(serde_json::Value::as_f64)
            .filter(|n| n.is_finite())
            .map(|n| n.clamp(0., 100.).round() as u8);
        Self { phase, percent }
    }
    /// Yoto has given up on this conversion; waiting longer cannot help.
    pub fn failed(&self) -> bool {
        self.phase.as_deref().is_some_and(|phase| {
            matches!(
                phase.to_ascii_lowercase().as_str(),
                "failed" | "failure" | "error" | "errored" | "cancelled" | "canceled" | "aborted"
            )
        })
    }
    fn reported(&self) -> bool {
        self.phase.is_some() || self.percent.is_some()
    }
}

/// Least and most time allowed for one conversion.
pub const BUDGET_MIN: Duration = Duration::from_secs(6 * 60);
pub const BUDGET_MAX: Duration = Duration::from_secs(20 * 60);
/// Progress is not judged during the first minutes, while Yoto queues the job.
pub const STALL_GRACE: Duration = Duration::from_secs(2 * 60);
/// Progress that has not moved for this long means the conversion is stuck.
pub const STALL_LIMIT: Duration = Duration::from_secs(4 * 60);

/// Time allowed to convert a track: 30 seconds per minute of audio, or 10 seconds per
/// megabyte when that is longer, kept between 6 and 20 minutes.
pub fn budget(audio_seconds: f64, bytes: u64) -> Duration {
    let by_duration = if audio_seconds.is_finite() && audio_seconds > 0. {
        Duration::from_secs_f64(audio_seconds / 2.)
    } else {
        Duration::ZERO
    };
    let by_size = Duration::from_secs(bytes / 100_000);
    by_duration.max(by_size).clamp(BUDGET_MIN, BUDGET_MAX)
}

/// Pause before asking Yoto again: frequent at first, then calmer for long conversions.
pub fn poll_interval(elapsed: Duration) -> Duration {
    if elapsed < Duration::from_secs(30) {
        Duration::from_secs(2)
    } else if elapsed < Duration::from_secs(120) {
        Duration::from_secs(4)
    } else {
        Duration::from_secs(6)
    }
}

/// What to do after a progress reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Wait,
    Stalled,
    OutOfTime,
}

/// Follows one conversion: its time budget and when its progress last moved.
#[derive(Debug, Clone)]
pub struct Watch {
    budget: Duration,
    last: Progress,
    moved_at: Duration,
}
impl Watch {
    pub fn new(budget: Duration) -> Self {
        Self {
            budget,
            last: Progress::default(),
            moved_at: Duration::ZERO,
        }
    }
    /// Record a reading taken `elapsed` after the wait started.
    ///
    /// A stall is only declared once Yoto has reported some progress: without any, a slow
    /// conversion and a stuck one look the same, and only the time budget applies.
    pub fn observe(&mut self, elapsed: Duration, progress: &Progress) -> Verdict {
        if *progress != self.last {
            self.last = progress.clone();
            self.moved_at = elapsed;
        }
        if elapsed >= self.budget {
            Verdict::OutOfTime
        } else if self.last.reported()
            && elapsed >= STALL_GRACE
            && elapsed.saturating_sub(self.moved_at) >= STALL_LIMIT
        {
            Verdict::Stalled
        } else {
            Verdict::Wait
        }
    }
    /// Last percentage Yoto reported, for messages and the job's progress.
    pub fn percent(&self) -> Option<u8> {
        self.last.percent
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn at(phase: &str, percent: u8) -> Progress {
        Progress {
            phase: Some(phase.into()),
            percent: Some(percent),
        }
    }
    const MIN: u64 = 60;
    #[test]
    fn the_budget_follows_the_track_within_bounds() {
        assert_eq!(budget(60., 1_000_000), BUDGET_MIN);
        assert_eq!(budget(30. * 60., 43_000_000), Duration::from_secs(15 * MIN));
        assert_eq!(budget(0., 90_000_000), Duration::from_secs(15 * MIN));
        assert_eq!(budget(60. * 60., 86_000_000), BUDGET_MAX);
        assert_eq!(budget(f64::NAN, 0), BUDGET_MIN);
    }
    #[test]
    fn progress_is_read_from_the_envelope() {
        let value = serde_json::json!({"progress":{"phase":"transcoding","percent":42.4}});
        assert_eq!(Progress::read(&value), at("transcoding", 42));
        let clamped = serde_json::json!({"progress":{"percent":180}});
        assert_eq!(Progress::read(&clamped).percent, Some(100));
        assert_eq!(
            Progress::read(&serde_json::json!({"transcodedSha256":null})),
            Progress::default()
        );
    }
    #[test]
    fn failed_phases_end_the_wait() {
        for phase in ["failed", "ERROR", "cancelled", "canceled", "aborted"] {
            assert!(at(phase, 10).failed(), "{phase}");
        }
        assert!(!at("transcoding", 10).failed());
        assert!(!Progress::default().failed());
    }
    #[test]
    fn a_conversion_that_stops_moving_is_stalled() {
        let mut watch = Watch::new(BUDGET_MAX);
        let s = Duration::from_secs;
        assert_eq!(watch.observe(s(10), &at("transcoding", 5)), Verdict::Wait);
        assert_eq!(watch.observe(s(60), &at("transcoding", 20)), Verdict::Wait);
        assert_eq!(
            watch.observe(s(4 * MIN), &at("transcoding", 20)),
            Verdict::Wait
        );
        assert_eq!(
            watch.observe(s(5 * MIN), &at("transcoding", 20)),
            Verdict::Stalled
        );
        assert_eq!(watch.percent(), Some(20));
    }
    #[test]
    fn moving_progress_keeps_waiting_until_the_budget() {
        let mut watch = Watch::new(BUDGET_MIN);
        for step in 0..6u8 {
            let elapsed = Duration::from_secs(u64::from(step) * MIN);
            assert_eq!(
                watch.observe(elapsed, &at("transcoding", step * 10)),
                Verdict::Wait
            );
        }
        assert_eq!(
            watch.observe(BUDGET_MIN, &at("transcoding", 70)),
            Verdict::OutOfTime
        );
    }
    #[test]
    fn silence_is_never_called_a_stall() {
        let mut watch = Watch::new(BUDGET_MAX);
        let quiet = Progress::default();
        assert_eq!(
            watch.observe(Duration::from_secs(10 * MIN), &quiet),
            Verdict::Wait
        );
        assert_eq!(watch.observe(BUDGET_MAX, &quiet), Verdict::OutOfTime);
    }
    #[test]
    fn polling_slows_down_over_time() {
        assert_eq!(poll_interval(Duration::ZERO), Duration::from_secs(2));
        assert_eq!(
            poll_interval(Duration::from_secs(60)),
            Duration::from_secs(4)
        );
        assert_eq!(
            poll_interval(Duration::from_secs(600)),
            Duration::from_secs(6)
        );
    }
}
