//! Runs one (library, scenario) pair and measures it.

use crate::alloc;
use crate::common::{Bb, Outcome, Scenario};
use std::hint::black_box;
use std::marker::PhantomData;
use std::time::Instant;

pub struct Config {
    /// Ticks per timing sample for one agent.
    pub single_ticks: u32,
    pub agents: usize,
    /// Frames (one tick of every agent) per population timing sample.
    pub frames: u32,
    pub samples: usize,
}

pub struct Report {
    pub lib: &'static str,
    pub checksum: (Bb, [u64; 3]),
    pub ns_per_tick: f64,
    pub ns_per_agent_tick: f64,
    pub allocs_per_tick: f64,
    pub bytes_per_tick: f64,
    /// `size_of` the per-agent runtime.
    pub agent_inline: usize,
    /// Heap each agent keeps after construction.
    pub agent_heap: f64,
    pub build_allocs: f64,
    pub build_ns: f64,
    /// Heap above the built population while all agents tick.
    pub transient_peak: usize,
    /// `size_of` plus heap of the tree definition, when agents share one.
    pub shared: Option<usize>,
}

pub trait Measure {
    fn lib(&self) -> &'static str;
    fn scenario(&self) -> Scenario;
    fn run(&self, config: &Config) -> Report;
    /// Ticks one agent `ticks` times; for external profilers.
    fn ticks(&self, ticks: u64) -> Bb;
    fn trace(&self, ticks: u32) -> (Bb, [u64; 3]);
}

struct Entry<T, A, MakeTree, MakeAgent, Tick> {
    lib: &'static str,
    shared: bool,
    scenario: Scenario,
    make_tree: MakeTree,
    make_agent: MakeAgent,
    tick: Tick,
    _types: PhantomData<fn() -> (T, A)>,
}

/// `make_tree` runs once per scenario. `make_agent` gives each agent what it
/// needs besides its blackboard; `shared` says whether agents run the tree
/// `make_tree` built, rather than a copy of it.
pub fn entry<T: 'static, A: 'static>(
    lib: &'static str,
    shared: bool,
    scenario: Scenario,
    make_tree: impl Fn() -> T + 'static,
    make_agent: impl Fn(&T) -> A + 'static,
    tick: impl Fn(&T, &mut A, &mut Bb) -> Outcome + 'static,
) -> Box<dyn Measure> {
    Box::new(Entry {
        lib,
        shared,
        scenario,
        make_tree,
        make_agent,
        tick,
        _types: PhantomData,
    })
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

impl<T, A, MakeTree, MakeAgent, Tick> Entry<T, A, MakeTree, MakeAgent, Tick>
where
    MakeTree: Fn() -> T,
    MakeAgent: Fn(&T) -> A,
    Tick: Fn(&T, &mut A, &mut Bb) -> Outcome,
{
    #[inline(always)]
    fn step(&self, tree: &T, agent: &mut A, bb: &mut Bb) -> Outcome {
        bb.step_world();
        (self.tick)(black_box(tree), agent, bb)
    }
}

impl<T, A, MakeTree, MakeAgent, Tick> Measure for Entry<T, A, MakeTree, MakeAgent, Tick>
where
    MakeTree: Fn() -> T,
    MakeAgent: Fn(&T) -> A,
    Tick: Fn(&T, &mut A, &mut Bb) -> Outcome,
{
    fn lib(&self) -> &'static str {
        self.lib
    }

    fn scenario(&self) -> Scenario {
        self.scenario
    }

    fn trace(&self, ticks: u32) -> (Bb, [u64; 3]) {
        let tree = (self.make_tree)();
        let mut agent = (self.make_agent)(&tree);
        let mut bb = Bb::new(self.scenario, 0);
        let mut counts = [0; 3];
        for _ in 0..ticks {
            counts[self.step(&tree, &mut agent, &mut bb) as usize] += 1;
        }
        (bb, counts)
    }

    fn ticks(&self, ticks: u64) -> Bb {
        let tree = (self.make_tree)();
        let mut agent = (self.make_agent)(&tree);
        let mut bb = Bb::new(self.scenario, 0);
        for _ in 0..ticks {
            black_box(self.step(&tree, &mut agent, &mut bb));
        }
        bb
    }

    fn run(&self, config: &Config) -> Report {
        let checksum = self.trace(100_000);

        // Shared definition.
        let before = alloc::snapshot();
        let tree = (self.make_tree)();
        let shared = self
            .shared
            .then(|| size_of::<T>() + alloc::snapshot().live - before.live);

        // One agent: time and steady-state allocation.
        let mut agent = (self.make_agent)(&tree);
        let mut bb = Bb::new(self.scenario, 0);
        for _ in 0..config.single_ticks / 10 {
            black_box(self.step(&tree, &mut agent, &mut bb));
        }
        let ns_per_tick = median(
            (0..config.samples)
                .map(|_| {
                    let start = Instant::now();
                    for _ in 0..config.single_ticks {
                        black_box(self.step(&tree, &mut agent, &mut bb));
                    }
                    start.elapsed().as_nanos() as f64 / config.single_ticks as f64
                })
                .collect(),
        );
        let before = alloc::snapshot();
        let counted = 100_000;
        for _ in 0..counted {
            black_box(self.step(&tree, &mut agent, &mut bb));
        }
        let after = alloc::snapshot();
        let allocs_per_tick = (after.allocs - before.allocs) as f64 / counted as f64;
        let bytes_per_tick = (after.bytes - before.bytes) as f64 / counted as f64;
        drop(agent);

        // A population: construction cost, retained memory, and ticking out of cache.
        let n = config.agents;
        let mut population: Vec<(A, Bb)> = Vec::with_capacity(n);
        let before = alloc::snapshot();
        let start = Instant::now();
        for i in 0..n {
            // Stagger the world clocks so agents are not in lockstep.
            population.push((
                (self.make_agent)(&tree),
                Bb::new(self.scenario, i as u32 * 7919),
            ));
        }
        let build_ns = start.elapsed().as_nanos() as f64 / n as f64;
        let after = alloc::snapshot();
        let build_allocs = (after.allocs - before.allocs) as f64 / n as f64;
        let agent_heap = (after.live - before.live) as f64 / n as f64;

        let frame = |population: &mut Vec<(A, Bb)>| {
            for (agent, bb) in population.iter_mut() {
                black_box(self.step(&tree, agent, bb));
            }
        };
        for _ in 0..config.frames / 5 {
            frame(&mut population);
        }
        // Sample storage is allocated before peak tracking restarts.
        let mut samples = Vec::with_capacity(config.samples);
        alloc::reset_peak();
        let resting = alloc::snapshot().live;
        for _ in 0..config.samples {
            let start = Instant::now();
            for _ in 0..config.frames {
                frame(&mut population);
            }
            samples.push(start.elapsed().as_nanos() as f64 / (config.frames as usize * n) as f64);
        }
        let transient_peak = alloc::peak() - resting;
        let ns_per_agent_tick = median(samples);

        Report {
            lib: self.lib,
            checksum,
            ns_per_tick,
            ns_per_agent_tick,
            allocs_per_tick,
            bytes_per_tick,
            agent_inline: size_of::<A>(),
            agent_heap,
            build_allocs,
            build_ns,
            transient_peak,
            shared,
        }
    }
}
