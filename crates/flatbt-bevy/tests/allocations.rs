//! A warmed-up tick allocates nothing beyond Bevy's own executor, as long as
//! no act appears or goes.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use flatbt_bevy::prelude::*;

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

#[derive(Component, Default)]
struct Walker {
    step: u32,
}

#[derive(Component, Clone, Copy, PartialEq, Debug)]
struct WalkingTo(u32);

/// Always running, with an act whose value changes every tick: written in
/// place, never inserted or removed after the first tick.
fn walk() -> impl BehaviorNode<Walker, WalkingTo> {
    leaf(|walker: &mut Walker| {
        walker.step += 1;
        NodeResult::Running(WalkingTo(walker.step))
    })
}

fn walk_in_parallel() -> impl BehaviorNode<Walker, WalkingTo> {
    walk()
}

const FRAMES: usize = 128;

/// With a task pool, so schedules run on Bevy's multi-threaded executor, as
/// in a game.
fn app() -> App {
    let mut app = App::new();
    app.add_plugins(TaskPoolPlugin::default());
    app
}

/// What a game orders after the tick. Being ordered after a system that holds
/// `Commands` is what makes Bevy insert a sync point, and that costs a task
/// spawn every frame whether or not anything was queued.
fn carry_out(walkers: Query<&WalkingTo>) {
    for walking in walkers.iter() {
        core::hint::black_box(walking);
    }
}

fn measured(app: &mut App) -> usize {
    for _ in 0..64 {
        app.update();
    }
    ALLOCATIONS.store(0, Ordering::Relaxed);
    TRACKING.store(true, Ordering::Relaxed);
    for _ in 0..FRAMES {
        app.update();
    }
    TRACKING.store(false, Ordering::Relaxed);
    ALLOCATIONS.load(Ordering::Relaxed)
}

fn spawn_walkers(app: &mut App, spawn: impl Fn(&mut World)) {
    for _ in 0..100 {
        spawn(app.world_mut());
    }
}

// One test, so no other test's allocations land in the global count.
#[test]
fn steady_ticks_add_no_allocations() {
    // Plain systems stand in for the tick and its skipped apply step.
    let mut baseline = app();
    baseline.add_systems(Update, (|| {}, || {}, carry_out).chain());
    let baseline = measured(&mut baseline);

    let mut serial = app();
    serial
        .add_plugins(BehaviorPlugin::for_tree(walk))
        .add_systems(Update, carry_out.after(BehaviorSystems));
    spawn_walkers(&mut serial, |world| {
        world.spawn((Walker::default(), Behavior::for_tree(walk)));
    });
    let serial = measured(&mut serial);
    assert!(
        serial <= baseline + 4,
        "serial tick allocated: baseline={baseline}, tick={serial} over {FRAMES} frames"
    );

    // `par_iter_mut` spawns tasks of its own, so compare it against a
    // parallel query that does nothing.
    let mut parallel_baseline = app();
    parallel_baseline.add_systems(
        Update,
        (
            |mut walkers: Query<&mut Walker>| walkers.par_iter_mut().for_each(|_| {}),
            || {},
            carry_out,
        )
            .chain(),
    );
    spawn_walkers(&mut parallel_baseline, |world| {
        world.spawn(Walker::default());
    });
    let parallel_baseline = measured(&mut parallel_baseline);

    let mut parallel = app();
    parallel
        .add_plugins(BehaviorPlugin::for_tree(walk_in_parallel).parallel())
        .add_systems(Update, carry_out.after(BehaviorSystems));
    spawn_walkers(&mut parallel, |world| {
        world.spawn((Walker::default(), Behavior::for_tree(walk_in_parallel)));
    });
    let parallel = measured(&mut parallel);
    assert!(
        parallel <= parallel_baseline + 4,
        "parallel tick allocated: baseline={parallel_baseline}, tick={parallel} over {FRAMES} frames"
    );
}
