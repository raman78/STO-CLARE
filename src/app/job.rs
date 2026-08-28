//! What the analysis thread is doing, for the window to show and — where it is
//! safe — stop.
//!
//! The thread already says *whether* it is working (the hourglass in the
//! toolbar). That is enough for a refresh, which takes as long as it takes and
//! cannot be interrupted anyway; it is not enough for clearing the log, which
//! reads every fight that is being kept out of the file, writes the file again
//! and reads the whole thing back. On a log of a year's play that is a minute
//! of a window showing a list of fights that no longer exist, with nothing
//! saying so.
//!
//! Shared as an `Arc` and read straight off atomics rather than sent down the
//! info channel: the drawing thread asks once per frame, and a channel that
//! reported progress would deliver it in a burst at the end — the worker only
//! reaches the channel between instructions.
//!
//! Cancelling is deliberately narrow. See [`Phase::can_cancel`].

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Which part of a job is running. Ordered the way they happen.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Phase {
    /// Nothing is running that the reader has to be told about.
    #[default]
    Idle,
    /// Reading the fights that are being kept out of the log. Counted, and the
    /// only phase that can be given up on.
    CopyingKept,
    /// Writing the log again without the deleted fights.
    RewritingLog,
    /// Reading the rewritten log back, which is what fills the list in again.
    ReadingLogAgain,
}

impl Phase {
    /// Whether the reader may still call it off.
    ///
    /// Only while the fights being kept are being read: nothing has been
    /// written at that point, so giving up leaves the log exactly as it was.
    /// Once the rewrite has started there is nothing to go back to — it
    /// replaces the file in one step (`rewrite_file`) — and the read that
    /// follows it is what puts the new list on screen, so stopping it would
    /// leave the window showing a log that no longer exists.
    pub fn can_cancel(self) -> bool {
        self == Phase::CopyingKept
    }

    /// What the window says is happening.
    pub fn label(self) -> &'static str {
        match self {
            Phase::Idle => "",
            Phase::CopyingKept => "Reading the fights that are being kept",
            Phase::RewritingLog => "Writing the log file again",
            Phase::ReadingLogAgain => "Reading the log back",
        }
    }

    fn from_u8(value: u8) -> Self {
        match value {
            1 => Phase::CopyingKept,
            2 => Phase::RewritingLog,
            3 => Phase::ReadingLogAgain,
            _ => Phase::Idle,
        }
    }

    fn as_u8(self) -> u8 {
        match self {
            Phase::Idle => 0,
            Phase::CopyingKept => 1,
            Phase::RewritingLog => 2,
            Phase::ReadingLogAgain => 3,
        }
    }
}

/// The live state, written by the analysis thread and read by the drawing one.
#[derive(Debug, Default)]
pub struct JobStatus {
    phase: AtomicUsize,
    done: AtomicUsize,
    /// `0` for a phase nothing can be counted in, which is what makes the
    /// window show a spinner rather than a bar it would have to make up.
    total: AtomicUsize,
    cancel: AtomicBool,
}

/// One frame's worth of it, read together so the window cannot draw a phase
/// from one moment and a count from another.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct JobProgress {
    pub phase: Phase,
    pub done: usize,
    pub total: usize,
}

impl JobProgress {
    pub fn is_running(self) -> bool {
        self.phase != Phase::Idle
    }

    /// How far along, where the phase is something that can be counted.
    pub fn fraction(self) -> Option<f32> {
        match self.total {
            0 => None,
            total => Some(self.done as f32 / total as f32),
        }
    }
}

impl JobStatus {
    /// Starts a phase. `total` is `0` where there is nothing to count.
    ///
    /// The cancel flag is cleared by [`Self::finish`] rather than here, so a
    /// press that lands between two phases is not thrown away by the phase
    /// change — the next check still sees it.
    pub fn start(&self, phase: Phase, total: usize) {
        self.done.store(0, Ordering::Relaxed);
        self.total.store(total, Ordering::Relaxed);
        self.phase.store(phase.as_u8() as usize, Ordering::Release);
    }

    pub fn progress(&self, done: usize) {
        self.done.store(done, Ordering::Relaxed);
    }

    /// Back to idle, which is what takes the window down.
    pub fn finish(&self) {
        self.cancel.store(false, Ordering::Relaxed);
        self.done.store(0, Ordering::Relaxed);
        self.total.store(0, Ordering::Relaxed);
        self.phase
            .store(Phase::Idle.as_u8() as usize, Ordering::Release);
    }

    /// Asks for the job to stop. Only honoured where the phase allows it; the
    /// worker decides, not the button.
    pub fn request_cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// Whether the reader has asked for this to stop *and* the phase running
    /// now is one that may. Called by the worker.
    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed) && self.progress_snapshot().phase.can_cancel()
    }

    pub fn progress_snapshot(&self) -> JobProgress {
        let phase = Phase::from_u8(self.phase.load(Ordering::Acquire) as u8);
        JobProgress {
            phase,
            done: self.done.load(Ordering::Relaxed),
            total: self.total.load(Ordering::Relaxed),
        }
    }

    /// Whether a cancel has been asked for, whatever the phase. What the window
    /// reads to say it is stopping.
    pub fn cancel_requested(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_job_starts_idle_and_ends_idle() {
        let status = JobStatus::default();
        assert!(!status.progress_snapshot().is_running());

        status.start(Phase::CopyingKept, 10);
        assert!(status.progress_snapshot().is_running());
        status.finish();
        assert!(!status.progress_snapshot().is_running());
    }

    #[test]
    fn a_counted_phase_reports_a_fraction_and_an_uncounted_one_does_not() {
        let status = JobStatus::default();
        status.start(Phase::CopyingKept, 4);
        status.progress(1);
        assert_eq!(Some(0.25), status.progress_snapshot().fraction());

        // Nothing to count: the window shows a spinner instead of inventing a
        // bar.
        status.start(Phase::RewritingLog, 0);
        assert_eq!(None, status.progress_snapshot().fraction());
    }

    /// The button may be pressed at any time; only the phase decides whether
    /// the worker acts on it. Writing the log has no half way to stop at.
    #[test]
    fn a_cancel_is_only_honoured_while_it_is_safe() {
        let status = JobStatus::default();
        status.start(Phase::CopyingKept, 10);
        status.request_cancel();
        assert!(status.cancelled());

        status.start(Phase::RewritingLog, 0);
        assert!(
            !status.cancelled(),
            "the file is being replaced; there is nothing to go back to"
        );
        assert!(
            status.cancel_requested(),
            "the press is not forgotten, it is only not acted on"
        );

        status.finish();
        assert!(!status.cancel_requested(), "a new job starts uncancelled");
    }
}
