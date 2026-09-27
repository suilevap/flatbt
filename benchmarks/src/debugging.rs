//! What FlatBT's tracing and inspection cost when used. `trace` records only
//! with debug assertions on (`flatbt::trace::ENABLED`); build with
//! `--profile debugging` for an optimized build that has them.

use crate::alloc;
use crate::common::{Bb, Scenario};
use flatbt::prelude::*;
use flatbt::trace::TraceLog;
use flatbt::{inspect, trace};
use std::fmt::Write as _;
use std::hint::black_box;
use std::time::Instant;

pub struct Row {
    pub scenario: Scenario,
    pub variant: &'static str,
    pub ns_per_tick: f64,
    pub allocs_per_tick: f64,
    pub bytes_per_tick: f64,
    /// Heap each agent keeps for tracing, after warming up.
    pub heap_per_agent: f64,
    /// Length of the text the variant writes each tick, if any.
    pub text_len: usize,
}

const WARM: u32 = 100_000;
const TICKS: u32 = 500_000;
const SAMPLES: usize = 5;

/// Times `tick` over `TICKS` ticks, median of `SAMPLES`, then counts its
/// allocations over another `TICKS`.
fn time(mut tick: impl FnMut()) -> (f64, f64, f64) {
    for _ in 0..WARM {
        tick();
    }
    let mut samples: Vec<f64> = (0..SAMPLES)
        .map(|_| {
            let start = Instant::now();
            for _ in 0..TICKS {
                tick();
            }
            start.elapsed().as_nanos() as f64 / TICKS as f64
        })
        .collect();
    samples.sort_by(f64::total_cmp);
    let before = alloc::snapshot();
    for _ in 0..TICKS {
        tick();
    }
    let after = alloc::snapshot();
    (
        samples[SAMPLES / 2],
        (after.allocs - before.allocs) as f64 / TICKS as f64,
        (after.bytes - before.bytes) as f64 / TICKS as f64,
    )
}

pub fn measure<N: BtNode<Bb>>(scenario: Scenario, make: fn() -> N) -> Vec<Row> {
    let tree = make();
    let mut rows = Vec::new();
    let mut row = |variant, (ns, allocs, bytes): (f64, f64, f64), heap: f64, text_len| {
        rows.push(Row {
            scenario,
            variant,
            ns_per_tick: ns,
            allocs_per_tick: allocs,
            bytes_per_tick: bytes,
            heap_per_agent: heap,
            text_len,
        })
    };

    // The update alone.
    let (mut slot, mut bb) = (None, Bb::new(scenario, 0));
    let plain = time(|| {
        bb.step_world();
        let _ = black_box(update_slot(&tree, &mut slot, &mut bb, EntryMode::Resume));
    });
    row("update", plain, 0.0, 0);

    // Every update traced into the agent's log.
    let (mut slot, mut bb) = (None, Bb::new(scenario, 0));
    let before = alloc::snapshot().live;
    let log = TraceLog::new();
    let traced = time(|| {
        bb.step_world();
        let _ = black_box(update_slot(
            &tree,
            &mut slot,
            &mut bb,
            log.entry(EntryMode::Resume),
        ));
    });
    let heap = (alloc::snapshot().live - before) as f64;
    row("update, traced", traced, heap, 0);

    // Traced, and the trace formatted every update, into a reused buffer.
    for (variant, alternate) in [
        ("traced + format `{}`", false),
        ("traced + format `{:#}`", true),
    ] {
        let (mut slot, mut bb) = (None, Bb::new(scenario, 0));
        let log = TraceLog::new();
        let mut text = String::with_capacity(4096);
        let mut len = 0;
        let cost = time(|| {
            bb.step_world();
            let _ = black_box(update_slot(
                &tree,
                &mut slot,
                &mut bb,
                log.entry(EntryMode::Resume),
            ));
            text.clear();
            let view = trace::trace(&tree, slot.as_ref(), &log);
            let _ = if alternate {
                write!(text, "{view:#}")
            } else {
                write!(text, "{view}")
            };
            len = text.len();
            black_box(&text);
        });
        row(variant, cost, 0.0, len);
    }

    // describe() every update, into a reused buffer.
    for (variant, alternate) in [
        ("update + describe `{}`", false),
        ("update + describe `{:#}`", true),
    ] {
        let (mut slot, mut bb) = (None, Bb::new(scenario, 0));
        let mut text = String::with_capacity(4096);
        let mut len = 0;
        let cost = time(|| {
            bb.step_world();
            let _ = black_box(update_slot(&tree, &mut slot, &mut bb, EntryMode::Resume));
            text.clear();
            let view = inspect::describe(&tree, slot.as_ref());
            let _ = if alternate {
                write!(text, "{view:#}")
            } else {
                write!(text, "{view}")
            };
            len = text.len();
            black_box(&text);
        });
        row(variant, cost, 0.0, len);
    }

    // path_id() every update: the cheap check for "did the decision change".
    let (mut slot, mut bb) = (None, Bb::new(scenario, 0));
    let cost = time(|| {
        bb.step_world();
        let _ = black_box(update_slot(&tree, &mut slot, &mut bb, EntryMode::Resume));
        black_box(inspect::path_id(&tree, slot.as_ref()));
    });
    row("update + path_id", cost, 0.0, 0);
    rows
}
