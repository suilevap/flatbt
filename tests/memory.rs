use flatbt::{BtNode, BtState, Entry, EntryMode, NodeResult, check, leaf, select, seq, update};

use NodeResult::{Failure, Success};

#[path = "support/reject.rs"]
mod reject;
use reject::Reject;

#[derive(Default)]
struct World {
    busy: bool,
    urgent: bool,
    /// Each update of a `Visits` node pushes its count of updates so far.
    seen: Vec<u32>,
}

/// Counts its updates in memory; running while the world is busy.
struct Visits;

impl BtNode<World> for Visits {
    type State = ();
    type Memory = u32;

    fn update(
        &self,
        _: &mut (),
        visits: &mut u32,
        world: &mut World,
        _: (),
        _: Entry<'_>,
    ) -> NodeResult {
        *visits += 1;
        world.seen.push(*visits);
        if world.busy {
            NodeResult::RUNNING
        } else {
            Success
        }
    }
}

#[test]
fn memory_survives_completion_and_restarts() {
    let tree = seq((Visits, Visits));
    let mut state = BtState::new(&tree);
    let mut world = World::default();
    for _ in 0..2 {
        assert_eq!(
            update(&tree, &mut state, &mut world, EntryMode::Evaluate),
            Success
        );
    }
    // Each child keeps its own count.
    assert_eq!(world.seen, [1, 1, 2, 2]);
}

#[test]
fn memory_survives_preemption() {
    let tree = select((check(|world: &World| world.urgent), Visits));
    let mut state = BtState::new(&tree);
    let mut world = World {
        busy: true,
        ..World::default()
    };
    let _ = update(&tree, &mut state, &mut world, EntryMode::Evaluate);
    world.urgent = true;
    assert_eq!(
        update(&tree, &mut state, &mut world, EntryMode::Evaluate),
        Success
    );
    world.urgent = false;
    let _ = update(&tree, &mut state, &mut world, EntryMode::Evaluate);
    assert_eq!(world.seen, [1, 2]);
}

#[test]
fn a_rejected_candidate_keeps_its_memory_writes() {
    let tree = select((Reject(Visits), leaf(|_: &mut World| Success)));
    let mut state = BtState::new(&tree);
    let mut world = World::default();
    for _ in 0..2 {
        assert_eq!(
            update(&tree, &mut state, &mut world, EntryMode::Evaluate),
            Success
        );
    }
    assert_eq!(world.seen, [1, 2]);
}

#[test]
fn reset_keeps_memory_and_forget_drops_it() {
    let tree = Visits;
    let mut state = BtState::new(&tree);
    let mut world = World::default();
    let _ = update(&tree, &mut state, &mut world, EntryMode::Evaluate);
    state.reset();
    let _ = update(&tree, &mut state, &mut world, EntryMode::Evaluate);
    state.forget();
    let _ = update(&tree, &mut state, &mut world, EntryMode::Evaluate);
    assert_eq!(world.seen, [1, 2, 1]);
}

#[test]
fn a_tree_without_memory_nodes_keeps_no_memory() {
    let tree = select((
        check(|world: &World| world.urgent),
        seq((
            leaf(|_: &mut World| Failure::<()>),
            leaf(|_: &mut World| Success),
        )),
    ));
    let state = BtState::new(&tree);
    assert_eq!(size_of_val(state.memory()), 0);
}
