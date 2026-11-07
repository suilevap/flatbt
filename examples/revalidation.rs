//! Reconsider planning tasks while retaining notes from a preempted plan.
//! These events describe reasoning, not completed movement or other world acts.

use flatbt::{BtState, update};
use flatbt::{EntryMode, NodeResult, check, leaf, select, seq};

#[path = "support/wait_frames.rs"]
mod wait;
use wait::wait_frames;

#[derive(Default)]
struct Agent {
    observed_urgent: bool,
    events: Vec<&'static str>,
}

fn main() {
    let tree = select((
        seq((
            check(|bb: &Agent| bb.observed_urgent),
            leaf(|bb: &mut Agent| {
                bb.events.push("start urgent planning");
                NodeResult::Success
            }),
            wait_frames(2),
            leaf(|bb: &mut Agent| {
                bb.events.push("finish urgent planning");
                NodeResult::Success
            }),
        )),
        seq((
            leaf(|bb: &mut Agent| {
                bb.events.push("start patrol planning");
                NodeResult::Success
            }),
            wait_frames(3),
            leaf(|bb: &mut Agent| {
                bb.events.push("finish patrol planning");
                NodeResult::Success
            }),
        )),
    ));
    let mut state: BtState<_, _> = BtState::new(&tree);
    let mut agent = Agent::default();
    for (update_index, urgent, mode) in [
        (1, false, EntryMode::Resume),
        (2, false, EntryMode::Evaluate),
        (3, true, EntryMode::Evaluate),
        (4, true, EntryMode::Resume),
        (5, true, EntryMode::Resume),
    ] {
        agent.observed_urgent = urgent; // Refresh the latest observation.
        let result = update(&tree, &mut state, &mut agent, mode);
        println!(
            "update {update_index}: {result:?} | {mode:?} | {:?}",
            agent.events
        );
    }
    assert_eq!(
        agent.events,
        [
            "start patrol planning",
            "start urgent planning",
            "finish urgent planning"
        ]
    );
    assert!(!state.is_running());
}
