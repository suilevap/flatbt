//! Thinking survives rejection; only the final act reaches the driver.

use flatbt::prelude::*;

#[path = "support/reject.rs"]
mod reject;
use reject::Reject;

#[derive(Default)]
struct Observed {
    position: u32,
}

#[derive(Default)]
struct Blackboard {
    observed: Observed,
    considered: Vec<(u32, u32)>,
}

#[derive(Debug, PartialEq)]
enum Act {
    WalkTo(u32),
}

struct World {
    position: u32,
    acts_applied: Vec<Act>,
}

struct ConsiderShortcut;

impl BtNode<Blackboard, Act> for ConsiderShortcut {
    type State = u32;
    type Memory = u32;

    fn update(
        &self,
        visits: &mut u32,
        attempts: &mut u32,
        bb: &mut Blackboard,
        _: (),
        _: Entry<'_>,
    ) -> NodeResult<Act> {
        *visits += 1;
        *attempts += 1;
        bb.considered.push((*visits, *attempts));
        NodeResult::Running(Act::WalkTo(100))
    }
}

#[test]
fn rejected_act_keeps_thoughts_and_memory_for_the_final_choice() {
    let tree = select((
        Reject(ConsiderShortcut),
        leaf(|bb: &mut Blackboard| {
            let (_, attempts) = bb.considered.last().copied().unwrap();
            NodeResult::Running(Act::WalkTo(attempts))
        }),
    ));
    let mut state = BtState::new(&tree);
    let mut bb = Blackboard::default();
    let mut world = World {
        position: 0,
        acts_applied: Vec::new(),
    };

    for attempt in 1..=2 {
        let previous_position = world.position;
        bb.observed.position = world.position; // Gather facts before thinking.
        let act = update(&tree, &mut state, &mut bb, EntryMode::Evaluate)
            .act()
            .unwrap();

        assert_eq!(act, Act::WalkTo(attempt));
        assert_eq!(bb.considered.last(), Some(&(1, attempt)));
        assert_eq!(world.position, previous_position);
        assert_eq!(bb.observed.position, previous_position);
        assert!(state.is_running());

        // The driver carries out the final choice, informed by the rejected one.
        let Act::WalkTo(to) = act;
        world.position = to;
        world.acts_applied.push(act);
        assert_eq!(bb.observed.position, previous_position);
    }

    assert_eq!(bb.considered, [(1, 1), (1, 2)]);
    assert_eq!(world.acts_applied, [Act::WalkTo(1), Act::WalkTo(2)]);
    assert_eq!(world.position, 2);
    assert_eq!(bb.observed.position, 1); // The last snapshot preceded the act.
    bb.observed.position = world.position;
    assert_eq!(bb.observed.position, 2);
    assert_eq!(bb.considered, [(1, 1), (1, 2)]);
}
