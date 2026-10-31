//! A global allocator that counts, so every library is measured the same way.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

pub struct Counting;

static ALLOCS: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

fn grow(bytes: usize) {
    ALLOCS.fetch_add(1, Relaxed);
    BYTES.fetch_add(bytes, Relaxed);
    let live = LIVE.fetch_add(bytes, Relaxed) + bytes;
    PEAK.fetch_max(live, Relaxed);
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        grow(layout.size());
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        grow(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        LIVE.fetch_sub(layout.size(), Relaxed);
        grow(new_size);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[derive(Clone, Copy)]
pub struct Snapshot {
    pub allocs: usize,
    pub bytes: usize,
    pub live: usize,
}

pub fn snapshot() -> Snapshot {
    Snapshot {
        allocs: ALLOCS.load(Relaxed),
        bytes: BYTES.load(Relaxed),
        live: LIVE.load(Relaxed),
    }
}

/// Restarts peak tracking from the current live size.
pub fn reset_peak() {
    PEAK.store(LIVE.load(Relaxed), Relaxed);
}

pub fn peak() -> usize {
    PEAK.load(Relaxed)
}
