//! Fixture-only Rust allocator observations. No formatting, allocation, or I/O
//! occurs in the allocator or snapshot path. Foreign/driver malloc is excluded.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};

struct ObservedAllocator;

#[global_allocator]
static ALLOCATOR: ObservedAllocator = ObservedAllocator;
// Keep the startup-only mode off the cache lines updated by every Rust thread.
// The 128-byte alignment matches the measurement host's cache line and avoids
// adding observer-switch false sharing to the on/off experiment.
#[repr(align(128))]
struct ObservationMode {
    enabled: AtomicBool,
    configured: AtomicBool,
}

static MODE: ObservationMode = ObservationMode {
    enabled: AtomicBool::new(true),
    configured: AtomicBool::new(false),
};

/// Configure once at fixture startup, before starting workers or measuring.
/// Leaving this uncalled preserves the existing enabled observation mode.
/// This is a diagnostic experiment; disabled heap evidence cannot pass the
/// maintained measured-heap contract.
#[allow(dead_code)] // The sibling output benchmark keeps its default observer.
pub fn configure(enabled: bool) -> Result<(), &'static str> {
    if MODE.configured.swap(true, Relaxed) {
        return Err("allocator observation must be configured only once at startup");
    }
    MODE.enabled.store(enabled, Relaxed);
    Ok(())
}

static ALLOC_CALLS: AtomicU64 = AtomicU64::new(0);
static ALLOC_BYTES: AtomicU64 = AtomicU64::new(0);
static DEALLOC_CALLS: AtomicU64 = AtomicU64::new(0);
static DEALLOC_BYTES: AtomicU64 = AtomicU64::new(0);
static REALLOC_CALLS: AtomicU64 = AtomicU64::new(0);
static REALLOC_OLD_BYTES: AtomicU64 = AtomicU64::new(0);
static REALLOC_NEW_BYTES: AtomicU64 = AtomicU64::new(0);

// SAFETY: Every operation delegates the original pointer/layout unchanged to
// System. Atomic observations neither allocate nor affect allocation lifetime.
unsafe impl GlobalAlloc for ObservedAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if MODE.enabled.load(Relaxed) {
            ALLOC_CALLS.fetch_add(1, Relaxed);
            ALLOC_BYTES.fetch_add(layout.size() as u64, Relaxed);
        }
        // SAFETY: Caller supplies GlobalAlloc's valid allocation layout.
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if MODE.enabled.load(Relaxed) {
            ALLOC_CALLS.fetch_add(1, Relaxed);
            ALLOC_BYTES.fetch_add(layout.size() as u64, Relaxed);
        }
        // SAFETY: Same layout contract as System::alloc_zeroed.
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        if MODE.enabled.load(Relaxed) {
            DEALLOC_CALLS.fetch_add(1, Relaxed);
            DEALLOC_BYTES.fetch_add(layout.size() as u64, Relaxed);
        }
        // SAFETY: Caller supplies the live pointer and its original layout.
        unsafe { System.dealloc(pointer, layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if MODE.enabled.load(Relaxed) {
            REALLOC_CALLS.fetch_add(1, Relaxed);
            REALLOC_OLD_BYTES.fetch_add(layout.size() as u64, Relaxed);
            REALLOC_NEW_BYTES.fetch_add(new_size as u64, Relaxed);
        }
        // SAFETY: Caller supplies GlobalAlloc's valid reallocation arguments.
        unsafe { System.realloc(pointer, layout, new_size) }
    }
}

#[derive(Clone, Copy, Default, serde::Serialize)]
pub struct Counts {
    alloc_calls: u64,
    alloc_requested_bytes: u64,
    dealloc_calls: u64,
    dealloc_bytes: u64,
    realloc_calls: u64,
    realloc_old_bytes: u64,
    realloc_requested_bytes: u64,
}

impl Counts {
    pub fn snapshot() -> Self {
        Self {
            alloc_calls: ALLOC_CALLS.load(Relaxed),
            alloc_requested_bytes: ALLOC_BYTES.load(Relaxed),
            dealloc_calls: DEALLOC_CALLS.load(Relaxed),
            dealloc_bytes: DEALLOC_BYTES.load(Relaxed),
            realloc_calls: REALLOC_CALLS.load(Relaxed),
            realloc_old_bytes: REALLOC_OLD_BYTES.load(Relaxed),
            realloc_requested_bytes: REALLOC_NEW_BYTES.load(Relaxed),
        }
    }
    pub fn observe(&mut self, before: Self, after: Self) {
        self.alloc_calls += after.alloc_calls - before.alloc_calls;
        self.alloc_requested_bytes += after.alloc_requested_bytes - before.alloc_requested_bytes;
        self.dealloc_calls += after.dealloc_calls - before.dealloc_calls;
        self.dealloc_bytes += after.dealloc_bytes - before.dealloc_bytes;
        self.realloc_calls += after.realloc_calls - before.realloc_calls;
        self.realloc_old_bytes += after.realloc_old_bytes - before.realloc_old_bytes;
        self.realloc_requested_bytes +=
            after.realloc_requested_bytes - before.realloc_requested_bytes;
    }
    pub fn report(&self, samples: usize, scope: &str) -> serde_json::Value {
        report_observation(self, samples, scope, MODE.enabled.load(Relaxed))
    }
}

fn report_observation(
    counts: &Counts,
    samples: usize,
    scope: &str,
    enabled: bool,
) -> serde_json::Value {
    if !enabled {
        return serde_json::json!({"status":"skipped","samples":0,"counts":null,
            "unobserved_frames":samples,"scope":scope,
            "allocator":"Rust System GlobalAlloc wrapper; observation disabled at startup",
            "limitations":"No allocator counts were observed in this mode. Diagnostic CPU timing only; this is not measured heap evidence."});
    }
    serde_json::json!({"status":"measured","samples":samples,"counts":counts,
        "scope":scope,"allocator":"Rust System GlobalAlloc wrapper; all process Rust threads within each sampled interval",
        "limitations":"requested calls/bytes, including failed requests; no live/peak heap, foreign malloc, Metal/driver allocations or VRAM; independent Relaxed field snapshots are not an atomic transaction; observer overhead included"})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_observer_reports_missing_evidence_instead_of_zero_allocations() {
        // Pure reporting path: never mutate global allocator mode in a parallel
        // test process whose other tests may allocate or sample counters.
        let counts = Counts {
            alloc_calls: 91,
            ..Default::default()
        };
        let off = report_observation(&counts, 240, "diagnostic frame interval", false);
        assert_eq!(off["status"], "skipped");
        assert_eq!(off["samples"], 0);
        assert_eq!(off["unobserved_frames"], 240);
        assert!(off["counts"].is_null());
        let on = report_observation(&counts, 240, "measured frame interval", true);
        assert_eq!(on["status"], "measured");
        assert_eq!(on["samples"], 240);
        assert_eq!(on["counts"]["alloc_calls"], 91);
        assert!(on.get("unobserved_frames").is_none());
        assert_eq!(std::mem::align_of::<ObservationMode>(), 128);
    }
}
