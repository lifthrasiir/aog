use std::io::Write;
use std::time::{Duration, Instant, SystemTime};

const PROGRESS_CHECK_RATIO: usize = 100;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(500);
const EMA_ALPHA: f64 = 0.3;

pub(crate) struct Progress {
    lcg_state: u64,
    last_report: Instant,
    solve_start: Instant,
    ema_nps: f64,
    last_report_nodes: u64,
    check_threshold: u64,
}

impl Progress {
    pub(crate) fn new() -> Self {
        let now = Instant::now();
        Self {
            lcg_state: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map_or(0x12345678_9ABCDEF0, |d| d.as_nanos() as u64),
            last_report: now,
            solve_start: now,
            ema_nps: 0.0,
            last_report_nodes: 0,
            check_threshold: u64::MAX,
        }
    }

    pub(crate) fn reset(&mut self, solve_start: Instant) {
        self.last_report = Instant::now();
        self.solve_start = solve_start;
        self.ema_nps = 0.0;
        self.last_report_nodes = 0;
        self.check_threshold = u64::MAX;
    }

    /// Returns `Some(elapsed_secs)` if a progress line should be printed.
    pub(crate) fn should_report(&mut self, node_count: u64) -> Option<f64> {
        self.lcg_state = self
            .lcg_state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1);
        if self.lcg_state <= self.check_threshold {
            let now = Instant::now();
            if now - self.last_report >= PROGRESS_INTERVAL {
                let elapsed = now - self.solve_start;
                let elapsed_secs = elapsed.as_secs_f64();

                let delta_nodes = node_count - self.last_report_nodes;
                let delta_secs = (now - self.last_report).as_secs_f64();
                let instant_nps = if delta_secs > 0.0 {
                    delta_nodes as f64 / delta_secs
                } else {
                    0.0
                };
                self.ema_nps = if self.ema_nps > 0.0 {
                    EMA_ALPHA * instant_nps + (1.0 - EMA_ALPHA) * self.ema_nps
                } else {
                    instant_nps
                };

                self.last_report = now;
                self.last_report_nodes = node_count;

                // Adaptively update check_threshold based on measured throughput.
                self.check_threshold = (u64::MAX as f64 * PROGRESS_CHECK_RATIO as f64
                    / self.ema_nps.max(PROGRESS_CHECK_RATIO as f64))
                    as u64;

                return Some(elapsed_secs);
            }
        }
        None
    }

    pub(crate) fn print(
        &self,
        elapsed_secs: f64,
        node_count: u64,
        curr_unknown: usize,
        total_unknown: usize,
    ) {
        eprint!(
            "\r{:>7.1}s  nodes: {:>10} | {:>8.0} n/s | remaining unknown: {:>4}/{:<4}",
            elapsed_secs, node_count, self.ema_nps, curr_unknown, total_unknown
        );
        let _ = std::io::stderr().flush();
    }

    pub(crate) fn clear_line() {
        eprint!("\r{:>80}\r", "");
    }
}
