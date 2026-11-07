//! Offload planning to a worker, retaining request-specific cancellation.
//! No gameplay movement occurs while the worker prepares its answer.

use flatbt::{BtState, EntryMode, NodeResult, action, check, select, seq, update};

#[path = "support/external_action.rs"]
mod external;
use external::{Agent, PlanExternally};

fn main() {
    let root = select((
        seq((
            check(|agent: &Agent| agent.urgent),
            action(PlanExternally {
                name: "urgent",
                frames: 3,
            }),
        )),
        action(PlanExternally {
            name: "patrol plan",
            frames: 12,
        }),
    ));
    let mut state: BtState<_, _> = BtState::new(&root);
    let mut agent = Agent::default();
    assert_eq!(
        update(&root, &mut state, &mut agent, EntryMode::Evaluate),
        NodeResult::RUNNING
    );
    let mut bt_updates = 1;
    println!("frame 0: BT starts a patrol plan");

    for frame in 1..=10 {
        if frame == 2 {
            agent.urgent = true;
        }
        let completed = agent.advance_planning();
        if let Some(planning) = &agent.planning {
            println!(
                "frame {frame}: {} has {} frames left",
                planning.name, planning.remaining_frames
            );
        }
        // Reevaluate periodically; resume early on completion of the current request.
        // Ignore stale events and idle trees.
        let mode = if frame % 4 == 0 {
            Some(EntryMode::Evaluate)
        } else if completed.is_some_and(|request| agent.is_current(request)) {
            Some(EntryMode::Resume)
        } else {
            None
        };
        if let Some(mode) = mode.filter(|_| state.is_running()) {
            let result = update(&root, &mut state, &mut agent, mode);
            bt_updates += 1;
            println!("frame {frame}: BT {mode:?} -> {result:?}");
        }
    }
    assert_eq!(bt_updates, 3); // Start, periodic preemption at 4, completion at 7.
    assert!(!state.is_running());
    assert!(agent.planning.is_none());
    println!("10 external frames; {bt_updates} BT updates including initial start");
}
