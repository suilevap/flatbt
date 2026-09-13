//! A control for the bench: what `par_iter_mut` can do on this machine, and
//! what the two ways of handing out `Commands` cost.
//!
//! The bench measures whole trees, which mixes the integration's overhead with
//! the machine's. This measures neither: a made-up per-entity workload, swept
//! from nothing to heavy, over three shapes -- plain `par_iter_mut`, a
//! `ParallelCommands` scope per entity, and one command queue per batch. It is
//! why `tick_behaviors_parallel` batches: at the cost of a real tree, the
//! per-entity scope costs more than it saves.
//!
//! ```sh
//! cargo run --release --bin scaling
//! ```
use std::time::Instant;

use bevy::app::TaskPoolThreadAssignmentPolicy;
use bevy::prelude::*;
use bevy::tasks::ComputeTaskPool;

#[derive(Component)]
struct Load(f32);

static ROUNDS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(200);

fn work(seed: f32) -> f32 {
    let mut acc = seed;
    for _ in 0..ROUNDS.load(std::sync::atomic::Ordering::Relaxed) {
        acc = acc * 1.000_001 + 0.5;
        acc = acc.sqrt().max(1.0);
    }
    acc
}

fn serial(mut q: Query<&mut Load>) {
    for mut load in q.iter_mut() {
        load.0 = work(load.0);
    }
}

fn parallel(mut q: Query<&mut Load>) {
    q.par_iter_mut().for_each(|mut load| load.0 = work(load.0));
}

/// One command queue per batch, handed back through Drop.
struct Batch<'a> {
    queue: bevy::ecs::world::CommandQueue,
    sink: &'a std::sync::Mutex<Vec<bevy::ecs::world::CommandQueue>>,
}

impl Drop for Batch<'_> {
    fn drop(&mut self) {
        self.sink
            .lock()
            .unwrap()
            .push(core::mem::take(&mut self.queue));
    }
}

fn parallel_batched(
    mut q: Query<&mut Load>,
    entities: &bevy::ecs::entity::Entities,
    allocator: &bevy::ecs::entity::EntityAllocator,
    mut commands: Commands,
) {
    let sink = std::sync::Mutex::new(Vec::new());
    q.par_iter_mut().for_each_init(
        || Batch {
            queue: default(),
            sink: &sink,
        },
        |batch, mut load| {
            let cmds = Commands::new_from_entities(&mut batch.queue, allocator, entities);
            core::hint::black_box(&cmds);
            load.0 = work(load.0);
        },
    );
    for mut queue in sink.into_inner().unwrap() {
        commands.append(&mut queue);
    }
}

/// Same, but taking a per-entity command scope the way the tick system does.
fn parallel_cmds(mut q: Query<&mut Load>, par: ParallelCommands) {
    q.par_iter_mut().for_each(|mut load| {
        par.command_scope(|_commands| load.0 = work(load.0));
    });
}

fn main() {
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    let pool = TaskPoolThreadAssignmentPolicy {
        min_threads: threads,
        max_threads: threads,
        percent: 1.0,
        on_thread_spawn: None,
        on_thread_destroy: None,
    };
    let mut app = App::new();
    app.add_plugins(MinimalPlugins.set(TaskPoolPlugin {
        task_pool_options: TaskPoolOptions {
            compute: pool,
            ..default()
        },
    }));
    let world = app.world_mut();
    for i in 0..100_000u32 {
        world.spawn(Load(i as f32));
    }
    app.update();
    println!("compute threads: {}", ComputeTaskPool::get().thread_num());

    println!(
        "{:>7}  {:>11}  {:>13}  {:>8}  {:>13}  {:>8}  {:>13}  {:>8}",
        "rounds",
        "serial ms",
        "parallel ms",
        "speedup",
        "par+cmds ms",
        "speedup",
        "batched ms",
        "speedup"
    );
    let _ = app.world_mut().run_system_cached(parallel_batched);
    for rounds in [0u32, 1, 2, 4, 8, 16, 32, 64, 200] {
        ROUNDS.store(rounds, std::sync::atomic::Ordering::Relaxed);
        for _ in 0..5 {
            let _ = app.world_mut().run_system_cached(serial);
            let _ = app.world_mut().run_system_cached(parallel);
        }
        let t = Instant::now();
        for _ in 0..20 {
            let _ = app.world_mut().run_system_cached(serial);
        }
        let serial_ms = t.elapsed().as_secs_f64() * 1000.0 / 20.0;
        let t = Instant::now();
        for _ in 0..20 {
            let _ = app.world_mut().run_system_cached(parallel);
        }
        let par_ms = t.elapsed().as_secs_f64() * 1000.0 / 20.0;
        let t = Instant::now();
        for _ in 0..20 {
            let _ = app.world_mut().run_system_cached(parallel_cmds);
        }
        let cmd_ms = t.elapsed().as_secs_f64() * 1000.0 / 20.0;
        let t = Instant::now();
        for _ in 0..20 {
            let _ = app.world_mut().run_system_cached(parallel_batched);
        }
        let batch_ms = t.elapsed().as_secs_f64() * 1000.0 / 20.0;
        println!(
            "{rounds:>7}  {serial_ms:>11.3}  {par_ms:>13.3}  {:>7.2}x  {cmd_ms:>13.3}  {:>7.2}x  \
             {batch_ms:>13.3}  {:>7.2}x",
            serial_ms / par_ms,
            serial_ms / cmd_ms,
            serial_ms / batch_ms
        );
    }
}
