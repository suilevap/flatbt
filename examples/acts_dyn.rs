//! The same guard, with the act as an interface instead of an enum.
//!
//! Nothing in FlatBT constrains the act type, so it can be a `Box<dyn Act>`
//! whose implementations know how to apply themselves. The driver then has no
//! `match` at all, and a new kind of order is a new type -- no existing file
//! has to learn about it. The price is an allocation per update that decides.
//!
//! Compare with [`acts`](acts.rs), which is the same tree over an enum.
//!
//! ```sh
//! cargo run --example acts_dyn
//! ```

use flatbt::{BtAction, BtNode, BtState, EntryMode, action, check, select, seq, update};

#[path = "support/guard.rs"]
mod support;
use support::{Guard, World};

/// An order the guard can be given. Each one carries out itself.
trait Act {
    fn apply(&self, world: &mut World);
}

struct WalkTo(f32);

impl Act for WalkTo {
    fn apply(&self, world: &mut World) {
        world.step_towards(self.0);
    }
}

struct Firing;

impl Act for Firing {
    fn apply(&self, world: &mut World) {
        world.fire();
    }
}

struct Loading;

impl Act for Loading {
    fn apply(&self, world: &mut World) {
        world.load_one();
    }
}

/// The actions are the same as in `acts`; only what `tick` returns differs.
struct Reload;

impl BtAction<Guard, Box<dyn Act>> for Reload {
    type State = ();

    fn start(&self, guard: &mut Guard, _: ()) -> Option<()> {
        guard.considered.push("reload");
        Some(())
    }

    fn is_in_progress(&self, _: &(), guard: &Guard, _: ()) -> bool {
        guard.observed.ammo < guard.observed.magazine
    }

    fn tick(&self, _: &mut (), _: &mut Guard, _: ()) -> Box<dyn Act> {
        Box::new(Loading)
    }
}

struct Shoot;

impl BtAction<Guard, Box<dyn Act>> for Shoot {
    type State = ();

    fn start(&self, guard: &mut Guard, _: ()) -> Option<()> {
        guard.considered.push("shoot");
        Some(())
    }

    fn is_in_progress(&self, _: &(), guard: &Guard, _: ()) -> bool {
        guard.in_range() && !guard.dry()
    }

    fn tick(&self, _: &mut (), _: &mut Guard, _: ()) -> Box<dyn Act> {
        Box::new(Firing)
    }
}

struct Approach;

impl BtAction<Guard, Box<dyn Act>> for Approach {
    type State = ();

    fn start(&self, guard: &mut Guard, _: ()) -> Option<()> {
        guard.considered.push("approach");
        Some(())
    }

    fn is_in_progress(&self, _: &(), guard: &Guard, _: ()) -> bool {
        !guard.in_range()
    }

    fn tick(&self, _: &mut (), guard: &mut Guard, _: ()) -> Box<dyn Act> {
        Box::new(WalkTo(guard.observed.intruder))
    }
}

fn guard() -> impl BtNode<Guard, Box<dyn Act>> {
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

        let observed = world.observe();
        guard.observed = observed;
        let act = update(&tree, &mut state, &mut guard, EntryMode::Evaluate).act();
        assert_eq!(guard.observed, observed);
        assert_eq!(world.observe(), observed);

        // No match, and no place that has to know every kind of order.
        match act {
            Some(act) => act.apply(&mut world),
            None => world.trace.push("idle".into()),
        }
        assert_eq!(guard.observed, observed);
    }

    for line in &world.trace {
        println!("{line}");
    }
}
