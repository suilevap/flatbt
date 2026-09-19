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
use support::Guard;

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

    fn start(&self, _: &mut Guard, _: ()) -> Option<()> {
        Some(())
    }

    fn is_in_progress(&self, _: &(), guard: &Guard, _: ()) -> bool {
        guard.ammo < guard.magazine
    }

    fn tick(&self, _: &mut (), _: &mut Guard, _: ()) -> Act {
        Act::Loading
    }
}

/// Shoots while the intruder is in range and there are rounds left.
struct Shoot;

impl BtAction<Guard, Act> for Shoot {
    type State = ();

    fn start(&self, _: &mut Guard, _: ()) -> Option<()> {
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

    fn start(&self, _: &mut Guard, _: ()) -> Option<()> {
        Some(())
    }

    fn is_in_progress(&self, _: &(), guard: &Guard, _: ()) -> bool {
        !guard.in_range()
    }

    fn tick(&self, _: &mut (), guard: &mut Guard, _: ()) -> Act {
        Act::WalkTo(guard.intruder)
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
    let mut guard = Guard::new(1);

    for tick in 1..=10 {
        if tick == 5 {
            guard.intruder = 8.0; // the intruder moves; the walk follows it
            guard.trace.push("-- intruder moves to 8 --".into());
        }

        // The whole interface between the tree and the game is this value.
        match update(&tree, &mut state, &mut guard, EntryMode::Evaluate).act() {
            Some(Act::WalkTo(target)) => guard.step_towards(target),
            Some(Act::Firing) => guard.fire(),
            Some(Act::Loading) => guard.load_one(),
            // A tree that ended is not doing anything.
            None => guard.trace.push("idle".into()),
        }
    }

    for line in &guard.trace {
        println!("{line}");
    }
}
