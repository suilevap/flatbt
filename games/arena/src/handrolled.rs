//! The same tick, written by hand, to measure what the generated one is worth.
//!
//! `HANDROLLED=1 cargo run --release --bin bench` runs the trees through this
//! instead of `BehaviorPlugin`'s system. It measures the same as the generated
//! one, which is the point: the declaration is a system writer, not overhead.
//!
//! What it does not have is what the generated system knows -- command queues
//! handed out per `par_iter` batch, a write-back skipped for a tree that only
//! read, and an order that cannot be got wrong. It is serial only, so the
//! parallel column of the bench is meaningless under `HANDROLLED`.

use bevy::prelude::*;
use flatbt::bevy::BehaviorTree;
use flatbt::bevy::prelude::*;

use crate::ai::{Fighter, RETHINK};
use crate::world::{Ammo, Arena, CoverTarget, Health, Intent, Speed};

/// One tree, one system, written out. No `BehaviorContext` involved: the query
/// is an ordinary query, the gather is ordinary code, the write-back is a
/// component assignment.
/// Spelled out because clippy will not have the tuple inline -- which is the
/// job `#[derive(QueryData)]` does for the declared version, and one more thing
/// this has to do for itself.
type Agents<'w, 's, F> = Query<
    'w,
    's,
    (
        Entity,
        &'static mut Behavior<Fighter, F>,
        &'static Transform,
        &'static Health,
        &'static Ammo,
        &'static Speed,
        Option<&'static CoverTarget>,
        &'static mut Intent,
    ),
>;

pub fn tick_fighters<F: TreeBuilder<Fighter>>(
    tree: Res<BehaviorTree<Fighter, F>>,
    arena: Res<Arena>,
    mut agents: Agents<F>,
    mut commands: Commands,
) {
    let definition = tree.get();
    for (entity, mut behavior, transform, health, ammo, speed, cover, mut intent) in
        agents.iter_mut()
    {
        let snapshot = Fighter {
            position: transform.translation.truncate(),
            health: health.0,
            ammo: ammo.0,
            speed: speed.0,
            cover: cover.map(|c| c.0),
            player: arena.player,
            rethink: evaluate_every(RETHINK, arena.elapsed, arena.delta, entity)
                == EntryMode::Evaluate,
            intent: Intent::default(),
        };
        let mut bb = Blackboard::<Fighter>::new(entity, snapshot);
        let mode = if bb.rethink {
            EntryMode::Evaluate
        } else {
            EntryMode::Resume
        };
        behavior.tick(definition, &mut bb, mode);
        if bb.written() {
            intent.set_if_neq(bb.snapshot().intent);
        }
        if let Some(mut queue) = bb.take_queue() {
            commands.append(&mut queue);
        }
    }
}

/// Registering it has to be generic too: a builder's type cannot be written
/// down, so only a function that takes one can name the system it installs.
pub fn plugin<F: TreeBuilder<Fighter> + Copy>(builder: F) -> impl Plugin {
    move |app: &mut App| {
        app.insert_resource(BehaviorTree::<Fighter, F>::new(&builder, None))
            .add_systems(Update, tick_fighters::<F>.in_set(BehaviorSystems));
    }
}
