//! Perception is a snapshot; reasoning survives rejected options.
//! Only the final act changes actual ammunition.
//!
//! ```sh
//! cargo run --example thinking
//! ```

use flatbt::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Observed {
    ammo: u32,
}

#[derive(Default)]
struct Reasoning {
    fire_checks: u32,
    last_rejection: Option<&'static str>,
}

struct Blackboard {
    observed: Observed,
    reasoning: Reasoning,
}

#[derive(Debug, PartialEq)]
enum Act {
    Fire,
    Reload,
}

struct World {
    ammo: u32,
    shots_fired: u32,
}

impl World {
    fn observe(&self) -> Observed {
        Observed { ammo: self.ammo }
    }

    fn apply(&mut self, act: Act) {
        match act {
            Act::Fire => {
                self.ammo -= 1;
                self.shots_fired += 1;
            }
            Act::Reload => self.ammo = 3,
        }
    }
}

fn brain() -> impl BtNode<Blackboard, Act> {
    select((
        leaf(|bb: &mut Blackboard| {
            bb.reasoning.fire_checks += 1;
            if bb.observed.ammo == 0 {
                bb.reasoning.last_rejection = Some("observed an empty magazine");
                NodeResult::Failure
            } else {
                NodeResult::Running(Act::Fire)
            }
        }),
        leaf(|_: &mut Blackboard| NodeResult::Running(Act::Reload)),
    ))
}

fn main() {
    let tree = brain();
    let mut state = BtState::new(&tree);
    let mut world = World {
        ammo: 0,
        shots_fired: 0,
    };
    let mut bb = Blackboard {
        observed: world.observe(),
        reasoning: Reasoning::default(),
    };

    for (checks, expected) in [(1, Act::Reload), (2, Act::Fire)] {
        // Gather facts, retaining the AI's reasoning from previous updates.
        let observed = world.observe();
        bb.observed = observed;
        let act = update(&tree, &mut state, &mut bb, EntryMode::Evaluate)
            .act()
            .unwrap();
        assert_eq!(act, expected);
        assert_eq!(bb.reasoning.fire_checks, checks);
        assert_eq!(
            bb.reasoning.last_rejection,
            Some("observed an empty magazine")
        );
        assert_eq!(bb.observed, observed);
        assert_eq!(world.observe(), observed);

        println!("observed {} rounds; chose {act:?}", observed.ammo);
        world.apply(act);
        assert_eq!(bb.observed, observed); // Facts have not been gathered again yet.
    }
    assert_eq!(world.ammo, 2);
    assert_eq!(world.shots_fired, 1);
    assert_eq!(bb.observed.ammo, 3);
    bb.observed = world.observe();
    assert_eq!(bb.observed.ammo, 2);
}
