//! When the playlist editor writes to Yoto.
//!
//! Edits are grouped: typing waits for a short pause (or for the field to lose focus),
//! discrete actions such as moves or icon changes are coalesced into one write, and only
//! one write per playlist is on its way at a time. Whatever changes during that write is
//! sent right after it, as the latest state of the local draft.
//!
//! This is pure bookkeeping on instants handed in by the caller, so it can be tested with a
//! fake clock; the editor owns the timer and the actual write.
use std::time::{Duration, Instant};

/// Quiet time after the last keystroke before a text field is saved.
pub(super) const TEXT_DEBOUNCE: Duration = Duration::from_millis(1500);
/// Quiet time after a discrete action (move, icon, removal…) so a burst becomes one write.
pub(super) const DISCRETE_DEBOUNCE: Duration = Duration::from_millis(2000);
/// Longest a change waits while edits keep coming, so a long burst still reaches Yoto.
pub(super) const MAX_DELAY: Duration = Duration::from_secs(10);

/// Longest the app waits, when quitting, for pending edits to reach Yoto.
pub(in crate::ui) const QUIT_WAIT: Duration = Duration::from_secs(20);
/// How often the app checks, while quitting, whether they did.
pub(in crate::ui) const QUIT_POLL: Duration = Duration::from_millis(100);

/// What kind of edit was made, which sets how long to wait for the next one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Change {
    /// A keystroke in a text field.
    Text,
    /// A one-off action: reorder, icon, add or remove a track, chapter edits, menus.
    Discrete,
}
impl Change {
    fn quiet(self) -> Duration {
        match self {
            Self::Text => TEXT_DEBOUNCE,
            Self::Discrete => DISCRETE_DEBOUNCE,
        }
    }
}

/// Local edits not sent yet: when the burst began and when it should be written.
#[derive(Clone, Copy, Debug)]
struct Burst {
    began: Instant,
    due: Instant,
}

/// The write schedule of one playlist.
#[derive(Debug, Default)]
pub(super) struct Autosave {
    pending: Option<Burst>,
    in_flight: bool,
}
impl Autosave {
    /// An edit happened at `now`: (re)start the quiet time, within the burst's longest wait.
    pub(super) fn record(&mut self, change: Change, now: Instant) {
        let began = self.pending.map_or(now, |burst| burst.began);
        let due = (now + change.quiet()).min(began + MAX_DELAY);
        self.pending = Some(Burst { began, due });
    }
    /// Send what is pending as soon as possible: a field lost focus, Enter was pressed, or
    /// the parent is leaving the playlist.
    pub(super) fn flush(&mut self, now: Instant) {
        if let Some(burst) = &mut self.pending {
            burst.due = burst.due.min(now);
        }
    }
    /// Local edits are waiting to be written.
    pub(super) fn has_pending(&self) -> bool {
        self.pending.is_some()
    }
    /// A write is on its way to Yoto.
    pub(super) fn in_flight(&self) -> bool {
        self.in_flight
    }
    /// When the next write should start, if one can: never while another is in flight.
    pub(super) fn next_due(&self) -> Option<Instant> {
        if self.in_flight {
            None
        } else {
            self.pending.map(|burst| burst.due)
        }
    }
    /// Start the pending write if it is due. Everything pending goes into that one write;
    /// edits made from now on wait for it to finish.
    pub(super) fn take_due(&mut self, now: Instant) -> bool {
        if self.next_due().is_some_and(|due| due <= now) {
            self.pending = None;
            self.in_flight = true;
            true
        } else {
            false
        }
    }
    /// Another write to this playlist starts (adding media from the library, deletion):
    /// it counts as the one in flight, and pending edits wait for it.
    pub(super) fn begin(&mut self) {
        self.in_flight = true;
    }
    /// The write in flight ended, successfully or not. Edits made meanwhile are still
    /// pending and go next, as soon as their own quiet time is over.
    pub(super) fn finish(&mut self) {
        self.in_flight = false;
    }
    /// Forget pending edits (the parent reloaded the Yoto version or deleted the playlist).
    pub(super) fn discard(&mut self) {
        self.pending = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A clock the test moves by hand.
    struct FakeClock {
        start: Instant,
        elapsed: Duration,
    }
    impl FakeClock {
        fn new() -> Self {
            Self {
                start: Instant::now(),
                elapsed: Duration::ZERO,
            }
        }
        fn at(&mut self, millis: u64) -> Instant {
            self.elapsed = Duration::from_millis(millis);
            self.now()
        }
        fn now(&self) -> Instant {
            self.start + self.elapsed
        }
    }

    /// The instants (in ms) at which writes start when polled every 10 ms until `end`.
    fn writes(
        autosave: &mut Autosave,
        clock: &mut FakeClock,
        mut edits: Vec<(u64, Change)>,
        flushes: &[u64],
        write_takes: u64,
        end: u64,
    ) -> Vec<u64> {
        edits.reverse();
        let mut started = vec![];
        let mut finishes_at = None;
        for t in (0..=end).step_by(10) {
            let now = clock.at(t);
            if finishes_at == Some(t) {
                autosave.finish();
                finishes_at = None;
            }
            while edits.last().is_some_and(|(at, _)| *at == t) {
                let (_, change) = edits.pop().unwrap();
                autosave.record(change, now);
            }
            if flushes.contains(&t) {
                autosave.flush(now);
            }
            if autosave.take_due(now) {
                started.push(t);
                finishes_at = Some(t + write_takes);
            }
        }
        started
    }

    #[test]
    fn typing_is_written_once_after_the_last_keystroke() {
        let (mut autosave, mut clock) = (Autosave::default(), FakeClock::new());
        let keys = (0..=1000).step_by(100).map(|t| (t, Change::Text)).collect();
        assert_eq!(
            writes(&mut autosave, &mut clock, keys, &[], 300, 5000),
            vec![2500]
        );
    }

    #[test]
    fn leaving_the_field_writes_at_once() {
        let (mut autosave, mut clock) = (Autosave::default(), FakeClock::new());
        let keys = vec![(0, Change::Text), (200, Change::Text)];
        assert_eq!(
            writes(&mut autosave, &mut clock, keys, &[400], 300, 5000),
            vec![400]
        );
    }

    #[test]
    fn a_flush_with_nothing_pending_writes_nothing() {
        let (mut autosave, mut clock) = (Autosave::default(), FakeClock::new());
        assert!(writes(&mut autosave, &mut clock, vec![], &[0, 500], 300, 3000).is_empty());
        assert!(!autosave.has_pending());
    }

    #[test]
    fn three_moves_within_two_seconds_make_one_write() {
        let (mut autosave, mut clock) = (Autosave::default(), FakeClock::new());
        let moves = vec![
            (0, Change::Discrete),
            (700, Change::Discrete),
            (1400, Change::Discrete),
        ];
        assert_eq!(
            writes(&mut autosave, &mut clock, moves, &[], 300, 6000),
            vec![3400]
        );
    }

    #[test]
    fn a_never_ending_burst_is_still_written_after_the_longest_wait() {
        let (mut autosave, mut clock) = (Autosave::default(), FakeClock::new());
        let keys = (0..=14_000)
            .step_by(500)
            .map(|t| (t, Change::Text))
            .collect();
        let started = writes(&mut autosave, &mut clock, keys, &[], 300, 20_000);
        assert_eq!(started.first(), Some(&10_000));
    }

    #[test]
    fn edits_during_a_write_wait_for_it_then_go_right_after() {
        let (mut autosave, mut clock) = (Autosave::default(), FakeClock::new());
        // First write at 1500 takes 3 s; a move and an icon change land meanwhile.
        let edits = vec![
            (0, Change::Text),
            (1600, Change::Discrete),
            (1700, Change::Discrete),
        ];
        assert_eq!(
            writes(&mut autosave, &mut clock, edits, &[], 3000, 10_000),
            vec![1500, 4500]
        );
    }

    #[test]
    fn edits_still_settling_when_a_write_ends_keep_their_quiet_time() {
        let (mut autosave, mut clock) = (Autosave::default(), FakeClock::new());
        // The write started at 1500 ends at 1800, while the parent is still typing.
        let edits = vec![
            (0, Change::Text),
            (1700, Change::Text),
            (2600, Change::Text),
        ];
        assert_eq!(
            writes(&mut autosave, &mut clock, edits, &[], 300, 8000),
            vec![1500, 4100]
        );
    }

    #[test]
    fn only_one_write_is_in_flight() {
        let mut clock = FakeClock::new();
        let mut autosave = Autosave::default();
        autosave.record(Change::Discrete, clock.at(0));
        autosave.flush(clock.at(0));
        assert!(autosave.take_due(clock.now()));
        autosave.record(Change::Discrete, clock.at(10));
        autosave.flush(clock.at(10));
        assert_eq!(autosave.next_due(), None);
        assert!(!autosave.take_due(clock.at(60_000)));
        autosave.finish();
        assert!(autosave.take_due(clock.now()));
    }

    #[test]
    fn another_write_holds_pending_edits_until_it_ends() {
        let mut clock = FakeClock::new();
        let mut autosave = Autosave::default();
        autosave.begin();
        autosave.record(Change::Text, clock.at(0));
        assert!(!autosave.take_due(clock.at(5000)));
        autosave.finish();
        assert!(autosave.take_due(clock.now()));
    }

    #[test]
    fn discarded_edits_are_not_written() {
        let mut clock = FakeClock::new();
        let mut autosave = Autosave::default();
        autosave.record(Change::Discrete, clock.at(0));
        autosave.discard();
        assert!(!autosave.take_due(clock.at(5000)));
        assert!(!autosave.has_pending());
    }
}
