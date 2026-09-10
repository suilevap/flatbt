use flatbt::{BtState, EntryMode, NodeResult, action, check, select, seq, update};

#[path = "support/external_action.rs"]
mod external;
use external::{Agent, MoveExternally};

fn main() {
    let root = select((
        seq((
            check(|agent: &Agent| agent.urgent),
            action(MoveExternally {
                name: "urgent",
                frames: 3,
            }),
        )),
        action(MoveExternally {
            name: "patrol",
            frames: 12,
        }),
    ));
    let mut state = BtState::new(&root);
    let mut agent = Agent::default();
    assert_eq!(
        update(&root, &mut state, &mut agent, EntryMode::Evaluate),
        NodeResult::Running
    );
    let mut bt_updates = 1;
    println!("frame 0: BT starts patrol");

    for frame in 1..=10 {
        if frame == 2 {
            agent.urgent = true;
        }
        let completed = agent.advance_movement();
        if let Some(movement) = &agent.movement {
            println!(
                "frame {frame}: {} has {} frames left",
                movement.name, movement.remaining_frames
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
    assert!(agent.movement.is_none());
    println!("10 external frames; {bt_updates} BT updates including initial start");
}
