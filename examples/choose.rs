//! Choose an internal planning task from the AI's latest observed order.
//! Completion records a plan; gameplay would be carried out by a final Act.

use flatbt::prelude::*;

#[path = "support/wait_frames.rs"]
mod wait;
use wait::wait_frames;

#[derive(Clone, Copy)]
enum Order {
    Move,
    Attack,
    Idle,
}

struct Observed {
    order: Order,
    close_range: bool,
}

struct Blackboard {
    observed: Observed,
    plans: Vec<&'static str>,
}

fn main() {
    let tree = choose!(|bb: &Blackboard| match bb.observed.order {
        Order::Move => seq((
            wait_frames(2),
            leaf(|bb: &mut Blackboard| {
                bb.plans.push("movement plan ready");
                NodeResult::Success
            }),
        )),
        Order::Attack => choose!(|bb: &Blackboard| match bb.observed.close_range {
            true => leaf(|bb: &mut Blackboard| {
                bb.plans.push("close combat plan ready");
                NodeResult::Success
            }),
            false => seq((
                wait_frames(1),
                leaf(|bb: &mut Blackboard| {
                    bb.plans.push("ranged combat plan ready");
                    NodeResult::Success
                }),
            )),
        }),
        Order::Idle => leaf(|_: &mut Blackboard| NodeResult::RUNNING),
    });
    let mut state: BtState<_, _> = BtState::new(&tree);
    let mut bb = Blackboard {
        observed: Observed {
            order: Order::Move,
            close_range: false,
        },
        plans: Vec::new(),
    };
    assert_eq!(
        update(&tree, &mut state, &mut bb, EntryMode::Resume),
        NodeResult::RUNNING
    );

    bb.observed.order = Order::Attack;
    // Resume continues movement despite the changed order.
    assert_eq!(
        update(&tree, &mut state, &mut bb, EntryMode::Resume),
        NodeResult::RUNNING
    );
    // Evaluate selects attack and replaces the movement state.
    assert_eq!(
        update(&tree, &mut state, &mut bb, EntryMode::Evaluate),
        NodeResult::RUNNING
    );
    assert_eq!(
        update(&tree, &mut state, &mut bb, EntryMode::Resume),
        NodeResult::Success
    );

    assert_eq!(bb.plans, ["ranged combat plan ready"]);
    println!("{:?}", bb.plans);

    bb.observed.order = Order::Idle;
    assert_eq!(
        update(&tree, &mut state, &mut bb, EntryMode::Evaluate),
        NodeResult::RUNNING
    );
    state.reset();
}
