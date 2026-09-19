//! What the trees cost per frame, headless, at a few population sizes.
//!
//! Timed between `start_timing` and `stop_timing`, which bracket
//! [`BehaviorSystems`], so the first number is the behaviour ticks and nothing
//! else. The second is the whole frame, because work moved out of the tick --
//! the gather and the apply -- has to show up somewhere.
//!
//! ```sh
//! cargo run --release --bin bench
//! AGENTS=250000 cargo run --release --bin bench   # other population sizes
//! THREADS=2 cargo run --release --bin bench       # pin the compute pool
//! TREE=coward cargo run --release --bin bench     # one mind, not a mix
//! SPLIT=1 cargo run --release --bin bench         # one tree under three names
//! ```

use std::time::{Duration, Instant};

use arena::Mind;
use arena::ai::{Fighter, chaser, coward, pace, sniper};
use arena::world::{ARENA, Ammo, Arena, Cover, Health, Speed, track_arena};
use bevy::prelude::*;
use flatbt::bevy::prelude::*;

#[derive(Resource, Default)]
struct AiCost {
    started: Option<Instant>,
    total: Duration,
    frames: u32,
}

fn start_timing(mut cost: ResMut<AiCost>) {
    cost.started = Some(Instant::now());
}

fn stop_timing(mut cost: ResMut<AiCost>) {
    if let Some(started) = cost.started.take() {
        cost.total += started.elapsed();
        cost.frames += 1;
    }
}

/// A cheap deterministic spread, so runs compare.
fn scatter(index: u32) -> Vec2 {
    let hash = index.wrapping_mul(2_654_435_761);
    let x = (hash >> 16) as f32 / 65_536.0;
    let y = (hash & 0xffff) as f32 / 65_536.0;
    Vec2::new(x - 0.5, y - 0.5) * ARENA
}

/// Three names for one tree, so the same work splits across three resources,
/// three archetypes and three systems. What that split costs is the question
/// behind "should trees of the same type share a loop".
fn split_a() -> impl BehaviorNode<Fighter> {
    sniper()
}

fn split_b() -> impl BehaviorNode<Fighter> {
    sniper()
}

fn split_c() -> impl BehaviorNode<Fighter> {
    sniper()
}

fn build(agents: u32, parallel: bool) -> App {
    let only = std::env::var("TREE").unwrap_or_default();
    let threads = std::env::var("THREADS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get()));
    let mut app = App::new();
    app.add_plugins(MinimalPlugins.set(TaskPoolPlugin {
        task_pool_options: TaskPoolOptions {
            compute: bevy::app::TaskPoolThreadAssignmentPolicy {
                min_threads: threads,
                max_threads: threads,
                percent: 1.0,
                on_thread_spawn: None,
                on_thread_destroy: None,
            },
            ..TaskPoolOptions::with_num_threads(threads)
        },
    }))
    .add_plugins(arena::ai::support)
    .init_resource::<Arena>()
    .init_resource::<AiCost>()
    .add_systems(
        Update,
        (
            track_arena.before(start_timing),
            start_timing.before(BehaviorSystems),
            stop_timing.after(BehaviorSystems),
        ),
    );

    if std::env::var_os("SPLIT").is_some() {
        // One tree, three names. Everything else about the run is the same.
        add_trees(&mut app, parallel, (split_a, split_b, split_c));
        let world = app.world_mut();
        spawn_cover(world);
        for index in 0..agents {
            let body = agent_body(index);
            match index % 3 {
                0 => world.spawn((body, Behavior::for_tree(split_a))),
                1 => world.spawn((body, Behavior::for_tree(split_b))),
                _ => world.spawn((body, Behavior::for_tree(split_c))),
            };
        }
        return app;
    }

    add_trees(&mut app, parallel, (chaser, sniper, coward));
    let world = app.world_mut();
    spawn_cover(world);
    for index in 0..agents {
        let body = agent_body(index);
        let mind = match only.as_str() {
            "chaser" => Mind::Chaser,
            "sniper" => Mind::Sniper,
            "coward" => Mind::Coward,
            _ => Mind::nth(index),
        };
        match mind {
            Mind::Chaser => world.spawn((body, Behavior::for_tree(chaser))),
            Mind::Sniper => world.spawn((body, Behavior::for_tree(sniper))),
            Mind::Coward => world.spawn((body, Behavior::for_tree(coward))),
        };
    }
    app
}

/// One registration per tree, and the same entry mode for all three: a fighter
/// keeps what it is doing until its staggered slot comes round.
fn add_trees<A, B, C>(app: &mut App, parallel: bool, trees: (A, B, C))
where
    A: TreeBuilder<Fighter>,
    B: TreeBuilder<Fighter>,
    C: TreeBuilder<Fighter>,
{
    let plugins = (
        BehaviorPlugin::for_tree(trees.0).tick_mode(pace),
        BehaviorPlugin::for_tree(trees.1).tick_mode(pace),
        BehaviorPlugin::for_tree(trees.2).tick_mode(pace),
    );
    if parallel {
        app.add_plugins((
            plugins.0.parallel(),
            plugins.1.parallel(),
            plugins.2.parallel(),
        ));
    } else {
        app.add_plugins(plugins);
    }
}

fn spawn_cover(world: &mut World) {
    for index in 0..24 {
        world.spawn((
            Transform::from_translation(scatter(index * 7919).extend(0.0)),
            Cover,
        ));
    }
}

/// One agent's components, so every spawn path makes the same population.
fn agent_body(index: u32) -> impl Bundle {
    (
        Transform::from_translation(scatter(index).extend(0.0)),
        Health(100.0 - (index % 90) as f32),
        Ammo(index % 7),
        Speed(1.0 + (index % 3) as f32 * 0.4),
        Fighter::default(),
    )
}

/// Both what the trees cost and what the whole frame costs.
fn measure(agents: u32, parallel: bool, frames: u32) -> (f64, f64) {
    let mut app = build(agents, parallel);
    for _ in 0..8 {
        app.update(); // warm up: first frames allocate invocation state
    }
    app.world_mut().resource_mut::<AiCost>().total = Duration::ZERO;
    app.world_mut().resource_mut::<AiCost>().frames = 0;
    let wall = Instant::now();
    for _ in 0..frames {
        app.update();
    }
    let wall = wall.elapsed().as_secs_f64() * 1000.0 / frames as f64;
    let cost = app.world().resource::<AiCost>();
    (cost.total.as_secs_f64() * 1000.0 / cost.frames as f64, wall)
}

fn main() {
    println!(
        "{:>8}  {:>12}  {:>12}  {:>8}  {:>12}  {:>12}",
        "agents", "serial ms", "parallel ms", "speedup", "frame ms", "par frame ms"
    );
    let sizes: Vec<u32> = match std::env::var("AGENTS") {
        Ok(v) => v.split(',').filter_map(|n| n.trim().parse().ok()).collect(),
        Err(_) => vec![1_000, 10_000, 50_000, 100_000],
    };
    for agents in sizes {
        let frames = if agents > 20_000 { 60 } else { 240 };
        let (serial, serial_frame) = measure(agents, false, frames);
        let (parallel, parallel_frame) = measure(agents, true, frames);
        println!(
            "{agents:>8}  {serial:>10.3}  {parallel:>10.3}  {:>7.2}x  {serial_frame:>10.3}  \
             {parallel_frame:>10.3}",
            serial / parallel
        );
    }
    println!(
        "\nthreads: {}",
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(0)
    );
}
