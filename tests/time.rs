use flatbt::prelude::*;

use NodeResult::{Failure, Success};

#[derive(Default)]
struct World {
    turn: u32,
    /// What the next `attempt` leaf returns.
    succeeds: bool,
    attempts: u32,
    urgent: bool,
}

impl BtClock for World {
    type Instant = u32;
    type Duration = u32;
    fn now(&self) -> u32 {
        self.turn
    }
}

fn attempt(world: &mut World) -> NodeResult<&'static str> {
    world.attempts += 1;
    if world.succeeds { Success } else { Failure }
}

fn forever(_: &mut World) -> NodeResult<&'static str> {
    NodeResult::Running("busy")
}

fn run<N: BtNode<World, &'static str>>(
    tree: &N,
    state: &mut BtState<N, World, &'static str>,
    world: &mut World,
    turn: u32,
    mode: EntryMode,
) -> NodeResult<&'static str> {
    world.turn = turn;
    update(tree, state, world, mode)
}

#[test]
fn action_wait_reports_its_act_until_the_span_has_passed() {
    let tree = action_wait(3, |_: &World| "aim");
    let mut state = BtState::new(&tree);
    let mut world = World::default();
    let resume = EntryMode::Resume;
    assert_eq!(
        run(&tree, &mut state, &mut world, 10, resume),
        NodeResult::Running("aim")
    );
    assert_eq!(
        run(&tree, &mut state, &mut world, 12, resume),
        NodeResult::Running("aim")
    );
    assert_eq!(run(&tree, &mut state, &mut world, 13, resume), Success);
    // A new run waits again.
    assert_eq!(
        run(&tree, &mut state, &mut world, 14, resume),
        NodeResult::Running("aim")
    );
}

#[test]
fn a_zero_wait_succeeds_at_once() {
    let tree = action_wait(0, |_: &World| "aim");
    let mut state = BtState::new(&tree);
    let mut world = World::default();
    assert_eq!(
        run(&tree, &mut state, &mut world, 0, EntryMode::Resume),
        Success
    );
}

#[test]
fn timeout_fails_a_child_still_running_after_the_span() {
    let tree = timeout(2, leaf(forever));
    let mut state = BtState::new(&tree);
    let mut world = World::default();
    let evaluate = EntryMode::Evaluate;
    assert_eq!(
        run(&tree, &mut state, &mut world, 0, evaluate),
        NodeResult::Running("busy")
    );
    // Evaluate continues the run; the deadline holds.
    assert_eq!(
        run(&tree, &mut state, &mut world, 1, evaluate),
        NodeResult::Running("busy")
    );
    assert_eq!(run(&tree, &mut state, &mut world, 2, evaluate), Failure);
    // The next run starts a new span.
    assert_eq!(
        run(&tree, &mut state, &mut world, 3, evaluate),
        NodeResult::Running("busy")
    );
}

#[test]
fn timeout_passes_a_finished_child_through() {
    let tree = timeout(2, leaf(attempt));
    let mut state = BtState::new(&tree);
    let mut world = World {
        succeeds: true,
        ..World::default()
    };
    assert_eq!(
        run(&tree, &mut state, &mut world, 0, EntryMode::Evaluate),
        Success
    );
}

#[test]
fn cooldown_counts_from_each_try() {
    let tree = cooldown(3, leaf(attempt));
    let mut state = BtState::new(&tree);
    let mut world = World::default();
    let evaluate = EntryMode::Evaluate;
    assert_eq!(run(&tree, &mut state, &mut world, 0, evaluate), Failure);
    assert_eq!(run(&tree, &mut state, &mut world, 2, evaluate), Failure);
    assert_eq!(world.attempts, 1);
    assert_eq!(run(&tree, &mut state, &mut world, 3, evaluate), Failure);
    assert_eq!(world.attempts, 2);
}

#[test]
fn success_cooldown_counts_from_the_last_success() {
    let tree = success_cooldown(3, leaf(attempt));
    let mut state = BtState::new(&tree);
    let mut world = World::default();
    let evaluate = EntryMode::Evaluate;
    // Failures can be retried at once.
    assert_eq!(run(&tree, &mut state, &mut world, 0, evaluate), Failure);
    assert_eq!(run(&tree, &mut state, &mut world, 0, evaluate), Failure);
    world.succeeds = true;
    assert_eq!(run(&tree, &mut state, &mut world, 1, evaluate), Success);
    assert_eq!(run(&tree, &mut state, &mut world, 3, evaluate), Failure);
    assert_eq!(world.attempts, 3);
    assert_eq!(run(&tree, &mut state, &mut world, 4, evaluate), Success);
}

#[test]
fn a_cooling_child_lets_its_selector_fall_back() {
    let tree = select((
        cooldown(5, leaf(attempt)),
        leaf(|_: &mut World| NodeResult::Running("rest")),
    ));
    let mut state = BtState::new(&tree);
    let mut world = World {
        succeeds: true,
        ..World::default()
    };
    let evaluate = EntryMode::Evaluate;
    assert_eq!(run(&tree, &mut state, &mut world, 0, evaluate), Success);
    assert_eq!(
        run(&tree, &mut state, &mut world, 1, evaluate),
        NodeResult::Running("rest")
    );
    assert_eq!(run(&tree, &mut state, &mut world, 5, evaluate), Success);
    assert_eq!(world.attempts, 2);
}

#[test]
fn each_cooldown_and_each_agent_keeps_its_own_time() {
    let tree = seq((cooldown(3, leaf(attempt)), cooldown(3, leaf(attempt))));
    let mut first = BtState::new(&tree);
    let mut second = BtState::new(&tree);
    let mut world = World {
        succeeds: true,
        ..World::default()
    };
    assert_eq!(
        run(&tree, &mut first, &mut world, 0, EntryMode::Evaluate),
        Success
    );
    assert_eq!(world.attempts, 2);
    assert_eq!(
        run(&tree, &mut second, &mut world, 1, EntryMode::Evaluate),
        Success
    );
    assert_eq!(
        run(&tree, &mut first, &mut world, 1, EntryMode::Evaluate),
        Failure
    );
    assert_eq!(world.attempts, 4);
}

#[test]
fn a_running_child_is_not_interrupted_by_its_cooldown() {
    let tree = cooldown(1, leaf(forever));
    let mut state = BtState::new(&tree);
    let mut world = World::default();
    for turn in 0..5 {
        assert_eq!(
            run(&tree, &mut state, &mut world, turn, EntryMode::Evaluate),
            NodeResult::Running("busy")
        );
    }
}

#[test]
fn reevaluate_every_reconsiders_a_resumed_subtree_at_its_rate() {
    let tree = reevaluate_every(
        3,
        select((
            guard(
                |world: &World| world.urgent,
                leaf(|_: &mut World| NodeResult::Running("flee")),
            ),
            leaf(forever),
        )),
    );
    let mut state = BtState::new(&tree);
    let mut world = World::default();
    let resume = EntryMode::Resume;
    assert_eq!(
        run(&tree, &mut state, &mut world, 0, EntryMode::Evaluate),
        NodeResult::Running("busy")
    );
    world.urgent = true;
    assert_eq!(
        run(&tree, &mut state, &mut world, 2, resume),
        NodeResult::Running("busy")
    );
    assert_eq!(
        run(&tree, &mut state, &mut world, 3, resume),
        NodeResult::Running("flee")
    );
}

#[test]
fn describe_shows_a_cooldown_off_the_running_path() {
    let tree = select((
        cooldown(5, leaf(attempt)),
        leaf(|_: &mut World| NodeResult::Running("rest")),
    ));
    let mut state = BtState::new(&tree);
    let mut world = World::default();
    let _ = run(&tree, &mut state, &mut world, 4, EntryMode::Evaluate);
    assert_eq!(
        format!("{:#}", state.describe().with_inactive()),
        "* select\n  - cooldown {span: 5, last: 4}\n    - attempt (leaf)\n  * leaf"
    );
}
