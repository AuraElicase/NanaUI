//! Fixture-only Rust allocator observations. No formatting, allocation, or I/O
//! occurs in the allocator or snapshot path. Foreign/driver malloc is excluded.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

struct ObservedAllocator;

#[global_allocator]
static ALLOCATOR: ObservedAllocator = ObservedAllocator;
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
        ALLOC_CALLS.fetch_add(1, Relaxed);
        ALLOC_BYTES.fetch_add(layout.size() as u64, Relaxed);
        // SAFETY: Caller supplies GlobalAlloc's valid allocation layout.
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        ALLOC_CALLS.fetch_add(1, Relaxed);
        ALLOC_BYTES.fetch_add(layout.size() as u64, Relaxed);
        // SAFETY: Same layout contract as System::alloc_zeroed.
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        DEALLOC_CALLS.fetch_add(1, Relaxed);
        DEALLOC_BYTES.fetch_add(layout.size() as u64, Relaxed);
        // SAFETY: Caller supplies the live pointer and its original layout.
        unsafe { System.dealloc(pointer, layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        REALLOC_CALLS.fetch_add(1, Relaxed);
        REALLOC_OLD_BYTES.fetch_add(layout.size() as u64, Relaxed);
        REALLOC_NEW_BYTES.fetch_add(new_size as u64, Relaxed);
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
        serde_json::json!({"status":"measured","samples":samples,"counts":self,
            "scope":scope,"allocator":"Rust System GlobalAlloc wrapper; all process Rust threads within each sampled interval",
            "limitations":"requested calls/bytes, including failed requests; no live/peak heap, foreign malloc, Metal/driver allocations or VRAM; independent Relaxed field snapshots are not an atomic transaction; observer overhead included"})
    }
}
