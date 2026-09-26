//! A warmed-up traced update allocates nothing: the log is cleared and reused.
#![cfg(debug_assertions)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use flatbt::prelude::*;

struct Counting;

static TRACKING: AtomicBool = AtomicBool::new(false);
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if TRACKING.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if TRACKING.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

#[test]
fn traced_updates_reuse_the_log() {
    // Alternates between a branch that fails and one that runs, so each
    // update records a different number of calls.
    let tree = select((
        seq((
            check(|n: &u32| n.is_multiple_of(2)),
            check(|n: &u32| *n > 100),
        )),
        leaf(|n: &mut u32| {
            *n += 1;
            NodeResult::RUNNING
        }),
    ));
    let mut state: BtState<_, _> = BtState::new(&tree);
    let log = flatbt::trace::TraceLog::new();
    let mut n = 0;
    for _ in 0..4 {
        let _ = update(&tree, &mut state, &mut n, log.entry(EntryMode::Evaluate));
    }
    TRACKING.store(true, Ordering::Relaxed);
    for _ in 0..128 {
        let _ = update(&tree, &mut state, &mut n, log.entry(EntryMode::Evaluate));
    }
    TRACKING.store(false, Ordering::Relaxed);
    assert_eq!(ALLOCATIONS.load(Ordering::Relaxed), 0);
}
