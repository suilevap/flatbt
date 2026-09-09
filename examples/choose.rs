use flatbt::{BtState, EntryMode, NodeResult, choose, leaf, seq, update};

#[path = "support/wait_frames.rs"]
mod wait;
use wait::wait_frames;

#[derive(Clone, Copy)]
enum Order {
    Move,
    Attack,
    Idle,
}

struct Blackboard {
    order: Order,
    close_range: bool,
}

fn main() {
    let tree = choose!(|bb: &Blackboard| match bb.order {
        Order::Move => seq((
            wait_frames(2),
            leaf(|_: &mut Blackboard| {
                println!("Arrived");
                NodeResult::Success
            }),
        )),
        Order::Attack => choose!(|bb: &Blackboard| match bb.close_range {
            true => leaf(|_: &mut Blackboard| {
                println!("Melee attack");
                NodeResult::Success
            }),
            false => seq((
                wait_frames(1),
                leaf(|_: &mut Blackboard| {
                    println!("Ranged attack");
                    NodeResult::Success
                }),
            )),
        }),
        Order::Idle => leaf(|_: &mut Blackboard| NodeResult::Running),
    });
    let mut state = BtState::new(&tree);
    let mut bb = Blackboard {
        order: Order::Move,
        close_range: false,
    };
    assert_eq!(
        update(&tree, &mut state, &mut bb, EntryMode::Resume),
        NodeResult::Running
    );

    bb.order = Order::Attack;
    // Resume continues movement despite the changed order.
    assert_eq!(
        update(&tree, &mut state, &mut bb, EntryMode::Resume),
        NodeResult::Running
    );
    // Evaluate selects attack and replaces the movement state.
    assert_eq!(
        update(&tree, &mut state, &mut bb, EntryMode::Evaluate),
        NodeResult::Running
    );
    assert_eq!(
        update(&tree, &mut state, &mut bb, EntryMode::Resume),
        NodeResult::Success
    );

    bb.order = Order::Idle;
    assert_eq!(
        update(&tree, &mut state, &mut bb, EntryMode::Evaluate),
        NodeResult::Running
    );
    state.reset();
}
