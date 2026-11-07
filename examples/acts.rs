//! What a tree decides, as an enum the driver matches on.
//!
//! The tree never changes the world. It says what the guard should be doing,
//! and `main` -- standing in for whatever carries decisions out -- does it.
//! Nothing below `fn guard()` knows how far a step is or what a shot costs.
//!
//! ```sh
//! cargo run --example acts
//! ```

use flatbt::{BtAction, BtNode, BtState, EntryMode, action, check, select, seq, update};

#[path = "support/guard.rs"]
mod support;
use support::{Guard, World};

/// The guard's whole vocabulary. An `Act` is an order to the world, not a
/// change to it.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Act {
    WalkTo(f32),
    Firing,
    Loading,
}

/// Loads until the magazine is full. The tree does not know how many rounds
/// that is or how fast they go in -- `is_in_progress` just watches the world.
struct Reload;

impl BtAction<Guard, Act> for Reload {
    type State = ();

    fn start(&self, guard: &mut Guard, _: ()) -> Option<()> {
        guard.considered.push("reload");
        Some(())
    }

    fn is_in_progress(&self, _: &(), guard: &Guard, _: ()) -> bool {
        guard.observed.ammo < guard.observed.magazine
    }

    fn tick(&self, _: &mut (), _: &mut Guard, _: ()) -> Act {
        Act::Loading
    }
}

/// Shoots while the intruder is in range and there are rounds left.
struct Shoot;

impl BtAction<Guard, Act> for Shoot {
    type State = ();

    fn start(&self, guard: &mut Guard, _: ()) -> Option<()> {
        guard.considered.push("shoot");
        Some(())
    }

    fn is_in_progress(&self, _: &(), guard: &Guard, _: ()) -> bool {
        guard.in_range() && !guard.dry()
    }

    fn tick(&self, _: &mut (), _: &mut Guard, _: ()) -> Act {
        Act::Firing
    }
}

/// Walks to wherever the target is *now*, restating the order on every update
/// without ending. That is how a standing intent follows a moving target.
struct Approach;

impl BtAction<Guard, Act> for Approach {
    type State = ();

    fn start(&self, guard: &mut Guard, _: ()) -> Option<()> {
        guard.considered.push("approach");
        Some(())
    }

    fn is_in_progress(&self, _: &(), guard: &Guard, _: ()) -> bool {
        !guard.in_range()
    }

    fn tick(&self, _: &mut (), guard: &mut Guard, _: ()) -> Act {
        Act::WalkTo(guard.observed.intruder)
    }
}

/// `Act` is declared nowhere but here: it unifies from the actions, and the
/// `check`s never name it.
fn guard() -> impl BtNode<Guard, Act> {
    select((
        seq((check(Guard::dry), action(Reload))),
        seq((check(Guard::in_range), action(Shoot))),
        action(Approach),
    ))
}

fn main() {
    let tree = guard();
    let mut state = BtState::new(&tree);
    let mut world = World::new(1);
    let mut guard = Guard::default();

    for tick in 1..=10 {
        if tick == 5 {
            world.intruder = 8.0;
            world.trace.push("-- intruder moves to 8 --".into());
        }

        // Refresh facts, preserving the AI's reasoning from previous updates.
        let observed = world.observe();
        guard.observed = observed;
        let act = update(&tree, &mut state, &mut guard, EntryMode::Evaluate).act();
        assert_eq!(guard.observed, observed);
        assert_eq!(world.observe(), observed);

        // Only this final act changes gameplay state. The observation stays
        // unchanged until the driver gathers fresh facts on the next tick.
        match act {
            Some(Act::WalkTo(target)) => world.step_towards(target),
            Some(Act::Firing) => world.fire(),
            Some(Act::Loading) => world.load_one(),
            // A tree that ended is not doing anything.
            None => world.trace.push("idle".into()),
        }
        assert_eq!(guard.observed, observed);
    }

    for line in &world.trace {
        println!("{line}");
    }
}
