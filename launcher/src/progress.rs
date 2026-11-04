//! Progress reporting between long-running work (prefix preparation today, the Rust patcher later) and any front end.
//!
//! The work side only talks to a [`Sink`]; the GUI (or the CLI) implements it. Nothing here knows about Slint. Only
//! values that were actually measured are reported: `done`/`total` stay `None` when a step cannot be measured, and
//! rate/ETA are derived from real samples by [`RateMeter`]. A patcher must not invent progress.

use std::time::Duration;

/// What the launcher is doing right now (user-visible step names are German: [`Phase::label`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Checking,
    Downloading,
    Verifying,
    Installing,
}

impl Phase {
    pub fn label(self) -> &'static str {
        match self {
            Phase::Checking => "Prüfen",
            Phase::Downloading => "Herunterladen",
            Phase::Verifying => "Verifizieren",
            Phase::Installing => "Installieren",
        }
    }
}

/// A snapshot of one work step. `done`/`total` are bytes (or any unit the sender documents) and only set if measurable.
#[derive(Clone, Debug, PartialEq)]
pub struct Progress {
    pub phase: Phase,
    /// Short description of the concrete step, for example "Wine-Prefix wird erstellt".
    pub step: String,
    pub done: Option<u64>,
    pub total: Option<u64>,
    /// Bytes per second, if a rate could be measured.
    pub rate: Option<f64>,
    pub eta_secs: Option<u64>,
}

impl Progress {
    /// A step whose size is not known: shown as busy, never as a percentage.
    pub fn indeterminate(phase: Phase, step: impl Into<String>) -> Self {
        Progress { phase, step: step.into(), done: None, total: None, rate: None, eta_secs: None }
    }

    pub fn fraction(&self) -> Option<f32> {
        match (self.done, self.total) {
            (Some(d), Some(t)) if t > 0 => Some((d.min(t) as f64 / t as f64) as f32),
            _ => None,
        }
    }

    pub fn percent(&self) -> Option<u32> {
        self.fraction().map(|f| (f * 100.0).round() as u32)
    }

    /// One line for the status area, only with what is known: `63 % · 41,2 MB von 65,0 MB · 12,0 MB/s · noch 3 s`.
    pub fn detail(&self) -> String {
        let mut parts = Vec::new();
        if let Some(p) = self.percent() {
            parts.push(format!("{p} %"));
        }
        if let (Some(d), Some(t)) = (self.done, self.total) {
            parts.push(format!("{} von {}", format_bytes(d), format_bytes(t)));
        }
        if let Some(r) = self.rate {
            parts.push(format!("{}/s", format_bytes(r as u64)));
        }
        if let Some(e) = self.eta_secs {
            parts.push(format!("noch {}", format_duration(e)));
        }
        parts.join(" · ")
    }
}

/// How a piece of work ended. The texts are English fallbacks: `Failed` carries only the technical detail, and a front end
/// that translates adds its own framing (the step names of [`Progress`] are English message ids as well).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Finished(String),
    Failed(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Update {
    Progress(Progress),
    Done(Outcome),
}

/// The interface between the work and the front end. Implemented for closures.
pub trait Sink: Send + Sync {
    fn update(&self, update: Update);
}

impl<F: Fn(Update) + Send + Sync> Sink for F {
    fn update(&self, update: Update) {
        self(update)
    }
}

/// Sink that discards everything (CLI without progress output, tests).
pub struct Silent;
impl Sink for Silent {
    fn update(&self, _: Update) {}
}

/// Derives a smoothed transfer rate and ETA from real (done, elapsed) samples. Time is passed in, so it is testable.
#[derive(Default)]
pub struct RateMeter {
    last: Option<(u64, Duration)>,
    rate: Option<f64>,
}

impl RateMeter {
    /// Feed the total bytes done so far and the time since the start. Returns (rate, ETA seconds), both only when real.
    pub fn sample(&mut self, done: u64, total: Option<u64>, elapsed: Duration) -> (Option<f64>, Option<u64>) {
        if let Some((d0, t0)) = self.last {
            let dt = elapsed.saturating_sub(t0).as_secs_f64();
            if dt >= 0.2 && done >= d0 {
                let inst = (done - d0) as f64 / dt;
                self.rate = Some(match self.rate {
                    Some(r) => 0.7 * r + 0.3 * inst, // exponential smoothing
                    None => inst,
                });
                self.last = Some((done, elapsed));
            }
        } else {
            self.last = Some((done, elapsed));
        }
        let eta = match (self.rate, total) {
            (Some(r), Some(t)) if r > 1.0 && t > done => Some(((t - done) as f64 / r).ceil() as u64),
            _ => None,
        };
        (self.rate.filter(|r| *r > 0.0), eta)
    }
}

/// `41,2 MB` (decimal units, German decimal comma).
pub fn format_bytes(n: u64) -> String {
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1000.0 && i < U.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    if i == 0 {
        format!("{n} B")
    } else {
        format!("{v:.1} {}", U[i]).replace('.', ",")
    }
}

/// `3 s`, `1 min 5 s`, `2 h 4 min`.
pub fn format_duration(secs: u64) -> String {
    match secs {
        0..=59 => format!("{secs} s"),
        60..=3599 => format!("{} min {} s", secs / 60, secs % 60),
        _ => format!("{} h {} min", secs / 3600, (secs % 3600) / 60),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unmeasurable_steps_show_no_numbers() {
        let p = Progress::indeterminate(Phase::Installing, "Wine-Prefix wird erstellt");
        assert_eq!((p.fraction(), p.percent()), (None, None));
        assert_eq!(p.detail(), "");
        assert_eq!(Phase::Installing.label(), "Installieren");
    }

    #[test]
    fn measured_progress_is_formatted_with_only_what_is_known() {
        let mut p = Progress::indeterminate(Phase::Downloading, "x");
        p.done = Some(41_200_000);
        p.total = Some(65_000_000);
        assert_eq!(p.percent(), Some(63));
        assert_eq!(p.detail(), "63 % · 41,2 MB von 65,0 MB");
        p.rate = Some(12_000_000.0);
        p.eta_secs = Some(65);
        assert_eq!(p.detail(), "63 % · 41,2 MB von 65,0 MB · 12,0 MB/s · noch 1 min 5 s");
        p.done = Some(70_000_000);
        assert_eq!(p.fraction(), Some(1.0), "never above 100 %");
    }

    #[test]
    fn rate_and_eta_come_from_real_samples_only() {
        let mut m = RateMeter::default();
        assert_eq!(m.sample(0, Some(1000), Duration::from_secs(0)), (None, None), "no sample pair yet");
        let (r, e) = m.sample(500, Some(1000), Duration::from_secs(1));
        assert_eq!((r, e), (Some(500.0), Some(1)));
        let (r2, _) = m.sample(500, Some(1000), Duration::from_millis(1050));
        assert_eq!(r2, Some(500.0), "samples closer than 0.2 s are ignored");
        assert_eq!(m.sample(1000, None, Duration::from_secs(2)).1, None, "no total, no ETA");
    }

    #[test]
    fn units_and_durations() {
        assert_eq!(format_bytes(999), "999 B");
        assert_eq!(format_bytes(1_500), "1,5 KB");
        assert_eq!(format_bytes(2_000_000_000), "2,0 GB");
        assert_eq!(format_duration(7), "7 s");
        assert_eq!(format_duration(3700), "1 h 1 min");
    }

    #[test]
    fn closures_are_sinks() {
        use std::sync::{Arc, Mutex};
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s2 = seen.clone();
        let sink = move |u: Update| s2.lock().unwrap().push(u);
        sink.update(Update::Done(Outcome::Finished("fertig".into())));
        assert_eq!(seen.lock().unwrap().len(), 1);
    }
}
