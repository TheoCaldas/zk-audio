//! Process memory sampling for the benchmark suite.
//!
//! Proving is memory-hungry and the interesting figure is a *peak*, not an
//! end-of-phase reading: Nova's folding loop allocates and frees per step,
//! and `create_recursive_circuit` additionally drives the wasm witness
//! generator, so the high-water mark usually occurs inside a library call
//! that returns long after the allocation is gone. Snapshots at phase
//! boundaries would miss it entirely, so a background thread polls RSS
//! while a phase runs and keeps the maximum.
//!
//! Caveats that belong with every number this produces:
//!
//! - **Process-wide.** The figure covers the whole benchmark process,
//!   including the witness generator's allocations and anything else the
//!   binary holds, not just the prover's own working set.
//! - **Sampled.** A spike shorter than the poll interval can be missed, so
//!   a reported peak is a lower bound on the true peak.
//! - **Not a memory requirement.** RSS is resident pages, which the OS may
//!   grow or reclaim under pressure; it is a measurement of this run on
//!   this machine, not a minimum spec.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// How often the sampler reads RSS. 50ms is fine-grained enough to catch the
/// multi-second allocation plateaus that proving produces, while costing
/// essentially nothing (one syscall per tick).
pub const SAMPLE_INTERVAL: Duration = Duration::from_millis(50);

/// Current resident set size of this process, in bytes.
///
/// Returns `None` where the platform is unsupported or the query fails, so
/// callers degrade to "no memory data" rather than reporting a wrong number.
#[cfg(target_os = "macos")]
pub fn current_rss_bytes() -> Option<u64> {
    // mach task_basic_info via proc_pidinfo. Declared locally rather than
    // pulling in a mach crate: this is the one call we need.
    #[repr(C)]
    struct ProcTaskInfo {
        pti_virtual_size: u64,
        pti_resident_size: u64,
        pti_total_user: u64,
        pti_total_system: u64,
        pti_threads_user: u64,
        pti_threads_system: u64,
        pti_policy: i32,
        pti_faults: i32,
        pti_pageins: i32,
        pti_cow_faults: i32,
        pti_messages_sent: i32,
        pti_messages_received: i32,
        pti_syscalls_mach: i32,
        pti_syscalls_unix: i32,
        pti_csw: i32,
        pti_threadnum: i32,
        pti_numrunning: i32,
        pti_priority: i32,
    }

    const PROC_PIDTASKINFO: libc::c_int = 4;

    unsafe extern "C" {
        fn proc_pidinfo(
            pid: libc::c_int,
            flavor: libc::c_int,
            arg: u64,
            buffer: *mut libc::c_void,
            buffersize: libc::c_int,
        ) -> libc::c_int;
    }

    let mut info = std::mem::MaybeUninit::<ProcTaskInfo>::zeroed();
    let size = std::mem::size_of::<ProcTaskInfo>() as libc::c_int;
    let written = unsafe {
        proc_pidinfo(
            std::process::id() as libc::c_int,
            PROC_PIDTASKINFO,
            0,
            info.as_mut_ptr() as *mut libc::c_void,
            size,
        )
    };
    if written != size {
        return None;
    }
    Some(unsafe { info.assume_init() }.pti_resident_size)
}

#[cfg(target_os = "linux")]
pub fn current_rss_bytes() -> Option<u64> {
    // statm field 2 is resident pages.
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    let pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if page_size <= 0 {
        return None;
    }
    Some(pages * page_size as u64)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn current_rss_bytes() -> Option<u64> {
    None
}

/// Whether memory sampling works at all here. Used to mark results honestly
/// instead of recording zeros that look like measurements.
pub fn is_supported() -> bool {
    current_rss_bytes().is_some()
}

/// A running sampler. Holds the peak seen since it started; `stop` joins the
/// thread and returns that peak.
pub struct PeakSampler {
    peak: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
    supported: bool,
}

impl PeakSampler {
    /// Starts sampling immediately. The first reading is taken synchronously,
    /// so even a phase shorter than one interval yields a value.
    pub fn start() -> Self {
        let first = current_rss_bytes();
        let supported = first.is_some();
        let peak = Arc::new(AtomicU64::new(first.unwrap_or(0)));
        let stop = Arc::new(AtomicBool::new(false));

        let handle = if supported {
            let peak = Arc::clone(&peak);
            let stop = Arc::clone(&stop);
            Some(thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    if let Some(rss) = current_rss_bytes() {
                        peak.fetch_max(rss, Ordering::Relaxed);
                    }
                    thread::sleep(SAMPLE_INTERVAL);
                }
                // One final reading, so a peak reached just before the stop
                // signal is not lost to the sleep.
                if let Some(rss) = current_rss_bytes() {
                    peak.fetch_max(rss, Ordering::Relaxed);
                }
            }))
        } else {
            None
        };

        PeakSampler {
            peak,
            stop,
            handle,
            supported,
        }
    }

    /// Peak RSS in bytes observed so far, without stopping the sampler.
    pub fn peak_bytes(&self) -> Option<u64> {
        if self.supported {
            Some(self.peak.load(Ordering::Relaxed))
        } else {
            None
        }
    }

    /// Stops sampling and returns the peak RSS in bytes.
    pub fn stop(mut self) -> Option<u64> {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
        self.peak_bytes()
    }
}

impl Drop for PeakSampler {
    fn drop(&mut self) {
        // Make sure a sampler dropped on an error path does not leak its
        // thread for the rest of the sweep.
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// Bytes to megabytes, for reporting.
pub fn to_mb(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rss_reads_a_plausible_value() {
        let rss = current_rss_bytes().expect("RSS unavailable on this platform");
        // A running test binary is at least a megabyte and well under a
        // terabyte; this catches a unit mix-up (pages vs bytes) rather than
        // asserting an exact figure.
        assert!(
            rss > 1024 * 1024,
            "RSS {} B implausibly small - wrong units?",
            rss
        );
        assert!(rss < 1024u64.pow(4), "RSS {} B implausibly large", rss);
    }

    #[test]
    fn sampler_observes_a_large_allocation() {
        let sampler = PeakSampler::start();
        let before = sampler.peak_bytes().unwrap();

        // Touch every page so the allocation is actually resident, and keep
        // it alive past the sleep so the sampler has a chance to see it.
        let mut big: Vec<u8> = Vec::with_capacity(256 * 1024 * 1024);
        for i in 0..(256 * 1024 * 1024 / 4096) {
            big.push(0u8);
            big.resize((i + 1) * 4096, 1u8);
        }
        thread::sleep(SAMPLE_INTERVAL * 4);
        let touched = big.len();
        drop(big);

        let peak = sampler.stop().unwrap();
        assert!(touched > 0);
        assert!(
            peak > before,
            "peak {} did not exceed baseline {} after a 256MB allocation",
            peak, before
        );
    }

    #[test]
    fn sampler_is_usable_for_a_very_short_phase() {
        // The synchronous first reading means even a phase shorter than one
        // sample interval reports something rather than zero.
        let sampler = PeakSampler::start();
        let peak = sampler.stop().unwrap();
        assert!(peak > 0);
    }
}
