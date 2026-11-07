use NodeResult::{Failure, Success};

/// `Running` for a tree that decides nothing; see `NodeResult::RUNNING`.
#[allow(non_upper_case_globals)]
const Running: NodeResult = NodeResult::RUNNING;
use flatbt::{BtState, EntryMode, NodeResult, action, check, select, seq, update};

#[path = "../examples/support/external_action.rs"]
mod external;
use external::{Agent, PlanExternally};

#[test]
fn external_progress_needs_no_bt_ticks_and_completion_advances_the_sequence() {
    let root = seq((
        action(PlanExternally {
            name: "route plan",
            frames: 2,
        }),
        action(PlanExternally {
            name: "followup",
            frames: 1,
        }),
    ));
    let mut state: BtState<_, _> = BtState::new(&root);
    let mut agent = Agent::default();
    assert_eq!(
        update(&root, &mut state, &mut agent, EntryMode::Resume),
        Running
    );
    let first = agent.planning.as_ref().unwrap().request;
    assert_eq!(agent.planning.as_ref().unwrap().remaining_frames, 2);
    assert_eq!(agent.advance_planning(), None);
    assert_eq!(agent.advance_planning(), Some(first));
    assert!(state.is_running()); // External completion has not been observed yet.
    assert_eq!(
        update(&root, &mut state, &mut agent, EntryMode::Resume),
        Running
    );
    let planning = agent.planning.as_ref().unwrap();
    assert_eq!(planning.name, "followup");
    assert_ne!(planning.request, first);
    let second = planning.request;
    assert_eq!(agent.advance_planning(), Some(second));
    assert_eq!(
        update(&root, &mut state, &mut agent, EntryMode::Resume),
        Success
    );
    assert!(agent.planning.is_none());
    assert!(!state.is_running());
}

#[test]
fn preemption_cannot_cancel_the_replacement_and_reset_stops_external_work() {
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
            frames: 5,
        }),
    ));
    let mut state: BtState<_, _> = BtState::new(&root);
    let mut agent = Agent::default();
    assert_eq!(
        update(&root, &mut state, &mut agent, EntryMode::Resume),
        Running
    );
    let old = agent.planning.as_ref().unwrap().request;
    agent.advance_planning();
    // Reevaluate the same invocation: start must not reset external progress.
    assert_eq!(
        update(&root, &mut state, &mut agent, EntryMode::Evaluate),
        Running
    );
    assert!(agent.is_current(old));
    assert_eq!(agent.planning.as_ref().unwrap().remaining_frames, 4);
    agent.urgent = true;
    assert_eq!(
        update(&root, &mut state, &mut agent, EntryMode::Evaluate),
        Running
    );
    assert!(!agent.is_current(old)); // A stale completion event must not wake it.
    assert_eq!(agent.planning.as_ref().unwrap().name, "urgent");
    assert_eq!(agent.planning.as_ref().unwrap().remaining_frames, 3);
    assert_eq!(agent.advance_planning(), None);
    assert_eq!(agent.planning.as_ref().unwrap().remaining_frames, 2);
    state.reset();
    // Drop signals cancellation; the worker removes the planning job.
    assert_eq!(agent.advance_planning(), None);
    assert!(agent.planning.is_none());
    assert!(!state.is_running());
}

#[test]
fn externally_removed_work_is_not_reported_as_success() {
    let root = action(PlanExternally {
        name: "route plan",
        frames: 2,
    });
    let mut state: BtState<_, _> = BtState::new(&root);
    let mut agent = Agent::default();
    assert_eq!(
        update(&root, &mut state, &mut agent, EntryMode::Resume),
        Running
    );
    agent.planning = None; // The worker canceled or removed the computation.
    assert_eq!(
        update(&root, &mut state, &mut agent, EntryMode::Resume),
        Failure
    );
    assert!(!state.is_running());
}
