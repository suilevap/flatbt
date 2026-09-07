use flatbt::{BtState, NodeResult, leaf, seq};

#[path = "support/wait_frames.rs"]
mod wait;
use wait::wait_frames;

#[derive(Default)]
struct Agent {
    checks: usize,
    shots: usize,
}

fn main() {
    let tree = seq((
        leaf(|ctx: &mut Agent| {
            ctx.checks += 1;
            NodeResult::Success
        }),
        wait_frames(3),
        leaf(|ctx: &mut Agent| {
            ctx.shots += 1;
            NodeResult::Success
        }),
    ));
    let mut state = BtState::new(&tree);
    let mut agent = Agent::default();
    for update in 1..=4 {
        let result = state.update(&mut agent);
        println!(
            "update {update}: {result:?} | checks={} shots={}",
            agent.checks, agent.shots
        );
    }
    assert_eq!(agent.checks, 1);
    assert_eq!(agent.shots, 1);
    assert!(!state.is_running());
}
