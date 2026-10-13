//! Launch frecency: a ranking score from how often and how recently a directory was launched.
//! The constants and the formula are the fff-search 0.11 rules.
use std::collections::HashMap;

/// The store keeps at most this many launch times for each directory.
pub const MAX_LAUNCHES: usize = 128;
/// The store and the score ignore launches older than 30 days.
pub const MAX_AGE_SECONDS: u64 = 30 * SECONDS_PER_DAY;
const SECONDS_PER_DAY: u64 = 86_400;
/// ln(2) / 10: a launch counts half after 10 days.
const DECAY_PER_DAY: f64 = 0.0693;

/// Sums `exp(-0.0693 * days_ago)` over launches (Unix seconds) of the last 30 days.
pub fn score(launches: impl IntoIterator<Item = u64>, now: u64) -> u32 {
    // Future times count as now. A sum above 10 becomes `10 + sqrt(sum - 10)`.
    let cutoff = now.saturating_sub(MAX_AGE_SECONDS);
    let sum: f64 = launches
        .into_iter()
        .filter(|&at| at >= cutoff)
        .map(|at| (-DECAY_PER_DAY * now.saturating_sub(at) as f64 / SECONDS_PER_DAY as f64).exp())
        .sum();
    let normalized = if sum <= 10.0 {
        sum
    } else {
        10.0 + (sum - 10.0).sqrt()
    };
    normalized.round() as u32
}

/// Frecency scores by directory, in the canonical host form of the HOME index.
#[derive(Debug, Clone, Default)]
pub struct Frecency {
    /// Windows paths compare ASCII case-insensitively, so their keys are lowercase.
    windows: bool,
    scores: HashMap<String, u32>,
}

impl Frecency {
    /// Scores (directory, Unix seconds) pairs. Spellings of one directory add up.
    pub fn new<'a>(
        home: &str,
        now: u64,
        launches: impl IntoIterator<Item = (&'a str, u64)>,
    ) -> Self {
        let windows = crate::host_path::HostPath::parse(home).is_some_and(|p| p.is_windows());
        let mut times: HashMap<String, Vec<u64>> = HashMap::new();
        for (path, at) in launches {
            if let Some(path) = crate::search::normalize_in(path, home) {
                times.entry(Self::key(windows, path)).or_default().push(at);
            }
        }
        let scores = times
            .into_iter()
            .map(|(path, times)| (path, score(times, now)))
            .filter(|(_, score)| *score > 0)
            .collect();
        Self { windows, scores }
    }

    /// The score of an index directory, or 0 if it has no recent launches.
    pub fn get(&self, path: &str) -> u32 {
        if self.scores.is_empty() {
            return 0;
        }
        let score = if self.windows {
            self.scores.get(&path.to_ascii_lowercase())
        } else {
            self.scores.get(path)
        };
        score.copied().unwrap_or(0)
    }

    fn key(windows: bool, path: String) -> String {
        if windows {
            path.to_ascii_lowercase()
        } else {
            path
        }
    }
}
