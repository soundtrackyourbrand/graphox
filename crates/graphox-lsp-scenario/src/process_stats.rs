//! CPU time and memory of the server process, read from the OS rather than
//! reported by the server, so the numbers include every thread and every
//! runtime the server happens to use.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug)]
pub struct ProcessSnapshot {
    /// User plus system CPU time consumed since the process started.
    pub cpu: Duration,
    pub rss_bytes: u64,
    pub threads: u32,
}

#[cfg(target_os = "macos")]
// libc deprecates its Mach bindings in favour of the mach2 crate; the timebase
// query is the only one needed here.
#[allow(deprecated)]
pub fn snapshot(pid: u32) -> Option<ProcessSnapshot> {
    use std::sync::OnceLock;

    // `proc_taskinfo` reports times in Mach absolute time units, which are only
    // nanoseconds on Intel. Apple Silicon needs the timebase conversion.
    static TIMEBASE: OnceLock<(u64, u64)> = OnceLock::new();
    let (numer, denom) = *TIMEBASE.get_or_init(|| {
        let mut info = libc::mach_timebase_info { numer: 0, denom: 0 };
        unsafe { libc::mach_timebase_info(&mut info) };
        if info.denom == 0 {
            (1, 1)
        } else {
            (info.numer as u64, info.denom as u64)
        }
    });

    let mut info: libc::proc_taskinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_taskinfo>() as libc::c_int;
    let written = unsafe {
        libc::proc_pidinfo(
            pid as libc::c_int,
            libc::PROC_PIDTASKINFO,
            0,
            &mut info as *mut _ as *mut libc::c_void,
            size,
        )
    };
    if written != size {
        return None;
    }
    let ticks = info.pti_total_user + info.pti_total_system;
    Some(ProcessSnapshot {
        cpu: Duration::from_nanos((ticks as u128 * numer as u128 / denom as u128) as u64),
        rss_bytes: info.pti_resident_size,
        threads: info.pti_threadnum as u32,
    })
}

#[cfg(target_os = "linux")]
pub fn snapshot(pid: u32) -> Option<ProcessSnapshot> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // The command name is parenthesised and may contain spaces; fields are
    // counted from after its closing parenthesis.
    let rest = &stat[stat.rfind(')')? + 2..];
    let fields: Vec<&str> = rest.split_whitespace().collect();
    let ticks_per_sec = unsafe { libc::sysconf(libc::_SC_CLK_TCK) }.max(1) as u64;
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) }.max(1) as u64;
    let utime: u64 = fields.get(11)?.parse().ok()?;
    let stime: u64 = fields.get(12)?.parse().ok()?;
    let threads: u32 = fields.get(17)?.parse().ok()?;
    let rss_pages: u64 = fields.get(21)?.parse().ok()?;
    Some(ProcessSnapshot {
        cpu: Duration::from_nanos((utime + stime) * 1_000_000_000 / ticks_per_sec),
        rss_bytes: rss_pages * page_size,
        threads,
    })
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn snapshot(_pid: u32) -> Option<ProcessSnapshot> {
    None
}

#[derive(Clone, Copy)]
pub struct Sample {
    pub at: Instant,
    pub snapshot: ProcessSnapshot,
}

/// Samples a process on a fixed interval so CPU use can be read over any
/// window after the fact.
pub struct Sampler {
    samples: Arc<Mutex<Vec<Sample>>>,
    stop: Arc<AtomicBool>,
}

pub const SAMPLE_INTERVAL: Duration = Duration::from_millis(100);

impl Sampler {
    pub fn start(pid: u32) -> Self {
        let samples = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (thread_samples, thread_stop) = (samples.clone(), stop.clone());
        thread::spawn(move || {
            while !thread_stop.load(Ordering::Relaxed) {
                match snapshot(pid) {
                    Some(snapshot) => thread_samples.lock().unwrap().push(Sample {
                        at: Instant::now(),
                        snapshot,
                    }),
                    None => break, // process gone
                }
                thread::sleep(SAMPLE_INTERVAL);
            }
        });
        Self { samples, stop }
    }

    pub fn latest(&self) -> Option<Sample> {
        self.samples.lock().unwrap().last().copied()
    }

    /// The first sample at or after `at`.
    pub fn at_or_after(&self, at: Instant) -> Option<Sample> {
        let samples = self.samples.lock().unwrap();
        let idx = samples.partition_point(|s| s.at < at);
        samples.get(idx).copied()
    }

    /// Average cores used between two instants.
    pub fn cores_between(&self, from: Instant, to: Instant) -> Option<f64> {
        let a = self.at_or_after(from)?;
        let b = self.at_or_after(to).or_else(|| self.latest())?;
        let wall = b.at.saturating_duration_since(a.at).as_secs_f64();
        if wall <= 0.0 {
            return None;
        }
        Some((b.snapshot.cpu.saturating_sub(a.snapshot.cpu)).as_secs_f64() / wall)
    }

    /// CPU seconds consumed between two instants.
    pub fn cpu_between(&self, from: Instant, to: Instant) -> Duration {
        match (
            self.at_or_after(from),
            self.at_or_after(to).or_else(|| self.latest()),
        ) {
            (Some(a), Some(b)) => b.snapshot.cpu.saturating_sub(a.snapshot.cpu),
            _ => Duration::ZERO,
        }
    }

    /// Highest average cores over any `window` within `[from, to]`, or over
    /// the whole interval when it is shorter than `window`.
    pub fn peak_cores(&self, from: Instant, to: Instant, window: Duration) -> f64 {
        if to.saturating_duration_since(from) < window {
            return self.cores_between(from, to).unwrap_or(0.0);
        }
        let samples = self.samples.lock().unwrap();
        let start = samples.partition_point(|s| s.at < from);
        let end = samples.partition_point(|s| s.at <= to);
        let slice = &samples[start..end];
        let mut peak = 0.0f64;
        let mut j = 0;
        for i in 0..slice.len() {
            while j < slice.len() && slice[j].at.duration_since(slice[i].at) < window {
                j += 1;
            }
            let Some(b) = slice.get(j) else { break };
            let wall = b.at.duration_since(slice[i].at).as_secs_f64();
            let cpu = b.snapshot.cpu.saturating_sub(slice[i].snapshot.cpu);
            peak = peak.max(cpu.as_secs_f64() / wall);
        }
        peak
    }
}

impl Drop for Sampler {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}
