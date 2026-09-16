//! What the trees cost per frame, headless, at a few population sizes.
//!
//! Timed between `start_timing` and `stop_timing`, which bracket
//! [`BehaviorSystems`], so the number is the behaviour ticks and nothing else.
//!
//! ```sh
//! cargo run --release --bin bench
//! AGENTS=250000 cargo run --release --bin bench   # other population sizes
//! THREADS=2 cargo run --release --bin bench       # pin the compute pool
//! TREE=coward cargo run --release --bin bench     # one mind, not a mix
//! ```

use std::time::{Duration, Instant};

use arena::Mind;
use arena::ai::{Fighter, chaser, coward, sniper};
use arena::world::{ARENA, Ammo, Arena, Cover, Health, Speed, resolve_cover_requests, track_arena};
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
    .init_resource::<Arena>()
    .init_resource::<AiCost>()
    .add_systems(
        Update,
        (
            track_arena.before(start_timing),
            start_timing.before(BehaviorSystems),
            stop_timing.after(BehaviorSystems),
            resolve_cover_requests.after(stop_timing),
        ),
    );

    let split = std::env::var_os("SPLIT").is_some();
    if split {
        // One tree, three names. Everything else about the run is the same.
        if parallel {
            app.add_plugins((
                BehaviorPlugin::for_tree(split_a).parallel(),
                BehaviorPlugin::for_tree(split_b).parallel(),
                BehaviorPlugin::for_tree(split_c).parallel(),
            ));
        } else {
            app.add_plugins((
                BehaviorPlugin::for_tree(split_a),
                BehaviorPlugin::for_tree(split_b),
                BehaviorPlugin::for_tree(split_c),
            ));
        }
        let world = app.world_mut();
        for index in 0..24 {
            world.spawn((
                Transform::from_translation(scatter(index * 7919).extend(0.0)),
                Cover,
            ));
        }
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

    let plugins = (
        BehaviorPlugin::for_tree(chaser),
        BehaviorPlugin::for_tree(sniper),
        BehaviorPlugin::for_tree(coward),
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

    let world = app.world_mut();
    for index in 0..24 {
        world.spawn((
            Transform::from_translation(scatter(index * 7919).extend(0.0)),
            Cover,
        ));
    }
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

/// One agent's components, so both spawn paths make the same population.
fn agent_body(index: u32) -> impl Bundle {
    (
        Transform::from_translation(scatter(index).extend(0.0)),
        Health(100.0 - (index % 90) as f32),
        Ammo(index % 7),
        Speed(1.0 + (index % 3) as f32 * 0.4),
    )
}

fn measure(agents: u32, parallel: bool, frames: u32) -> f64 {
    let mut app = build(agents, parallel);
    for _ in 0..8 {
        app.update(); // warm up: first frames allocate invocation state
    }
    app.world_mut().resource_mut::<AiCost>().total = Duration::ZERO;
    app.world_mut().resource_mut::<AiCost>().frames = 0;
    for _ in 0..frames {
        app.update();
    }
    let cost = app.world().resource::<AiCost>();
    cost.total.as_secs_f64() * 1000.0 / cost.frames as f64
}

fn main() {
    println!(
        "{:>8}  {:>12}  {:>12}  {:>8}",
        "agents", "serial ms", "parallel ms", "speedup"
    );
    let sizes: Vec<u32> = match std::env::var("AGENTS") {
        Ok(v) => v.split(',').filter_map(|n| n.trim().parse().ok()).collect(),
        Err(_) => vec![1_000, 10_000, 50_000, 100_000],
    };
    for agents in sizes {
        let frames = if agents > 20_000 { 60 } else { 240 };
        let serial = measure(agents, false, frames);
        let parallel = measure(agents, true, frames);
        println!(
            "{agents:>8}  {serial:>12.3}  {parallel:>12.3}  {:>7.2}x",
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
