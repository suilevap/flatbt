//! Suspend an internal plan, then record that it is ready. No world changes.

use flatbt::{BtState, update};
use flatbt::{EntryMode, NodeResult, leaf, seq};

#[path = "support/wait_frames.rs"]
mod wait;
use wait::wait_frames;

#[derive(Default)]
struct Agent {
    checks: usize,
    plans_prepared: usize,
}

fn main() {
    let tree = seq((
        leaf(|bb: &mut Agent| {
            bb.checks += 1;
            NodeResult::Success
        }),
        wait_frames(3),
        leaf(|bb: &mut Agent| {
            bb.plans_prepared += 1;
            NodeResult::Success
        }),
    ));
    let mut state: BtState<_, _> = BtState::new(&tree);
    let mut agent = Agent::default();
    for update_index in 1..=4 {
        let result = update(&tree, &mut state, &mut agent, EntryMode::Resume);
        println!(
            "update {update_index}: {result:?} | checks={} plans_prepared={}",
            agent.checks, agent.plans_prepared
        );
    }
    assert_eq!(agent.checks, 1);
    assert_eq!(agent.plans_prepared, 1);
    assert!(!state.is_running());
}
