use flatbt::goal_match;
use flatbt::prelude::*;

use NodeResult::{Failure, Running, Success};

const DOOR: u32 = 5;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Goal {
    Reach(u32),
    OpenDoor,
    GetKey,
    Climb,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Act {
    Walk(u32),
    Climb,
}

struct World {
    target: u32,
    at: u32,
    door_open: bool,
    key_at: Option<u32>,
    has_key: bool,
    can_climb: bool,
    climbed: bool,
    /// Updates a climb takes.
    climb_left: u32,
    door_tries: u32,
}

impl World {
    fn new(target: u32) -> Self {
        Self {
            target,
            at: 0,
            door_open: false,
            key_at: Some(2),
            has_key: false,
            can_climb: false,
            climbed: false,
            climb_left: 0,
            door_tries: 0,
        }
    }

    fn door_between(&self, to: u32) -> bool {
        let (low, high) = (self.at.min(to), self.at.max(to));
        !self.door_open && !self.climbed && low < DOOR && DOOR <= high
    }
}

fn reach_target(goal: &Goal) -> u32 {
    match goal {
        Goal::Reach(to) => *to,
        _ => unreachable!("only Reach goals walk"),
    }
}

/// One step per update toward the goal's position.
fn walk(world: &mut World, goal: &Goal) -> NodeResult<Act> {
    let to = reach_target(goal);
    if world.at == to {
        return Success;
    }
    if to > world.at {
        world.at += 1
    } else {
        world.at -= 1
    }
    Running(Act::Walk(to))
}

fn tree<const N: usize>() -> impl BtNode<World, Act> {
    goals::<N, _, _>(
        |world: &World| Goal::Reach(world.target),
        goal_match!(|goal: &Goal| {
            Goal::Reach(_) => select((
                seq((
                    need(|world: &World, goal: &Goal| {
                        world.door_between(reach_target(goal)).then_some(Goal::OpenDoor)
                    }),
                    with_goal(leaf_with(walk)),
                )),
                seq((
                    need(|world: &World, goal: &Goal| {
                        world.door_between(reach_target(goal)).then_some(Goal::Climb)
                    }),
                    with_goal(leaf_with(walk)),
                )),
            )),
            Goal::OpenDoor => seq((
                leaf(|world: &mut World| {
                    world.door_tries += 1;
                    Success
                }),
                need(|world: &World, _: &Goal| (!world.has_key).then_some(Goal::GetKey)),
                leaf(|world: &mut World| {
                    world.door_open = true;
                    Success
                }),
            )),
            Goal::GetKey => seq((
                need(|world: &World, _: &Goal| world.key_at.map(Goal::Reach)),
                leaf(|world: &mut World| {
                    if world.key_at == Some(world.at) {
                        world.has_key = true;
                        Success
                    } else {
                        Failure
                    }
                }),
            )),
            Goal::Climb => leaf(|world: &mut World| {
                if !world.can_climb {
                    Failure
                } else if world.climb_left > 0 {
                    world.climb_left -= 1;
                    Running(Act::Climb)
                } else {
                    world.climbed = true;
                    Success
                }
            }),
        }),
    )
}

fn run<N: BtNode<World, Act>>(
    tree: &N,
    state: &mut BtState<N, World, Act>,
    world: &mut World,
) -> NodeResult<Act> {
    update(tree, state, world, EntryMode::Evaluate)
}

#[test]
fn a_blocker_becomes_a_chain_of_subgoals() {
    let tree = tree::<8>();
    let mut state = BtState::new(&tree);
    let mut world = World::new(7);
    // Reach(7) needs the door open, which needs the key at 2.
    assert_eq!(run(&tree, &mut state, &mut world), Running(Act::Walk(2)));
    assert_eq!(
        state.describe().to_string().split(" > ").next(),
        Some("goals {stack: [Reach(7), OpenDoor, GetKey, Reach(2)]}")
    );
    assert_eq!(run(&tree, &mut state, &mut world), Running(Act::Walk(2)));
    // At the key: picked up, door opened, and on toward the target, in one update.
    assert_eq!(run(&tree, &mut state, &mut world), Running(Act::Walk(7)));
    assert!(world.has_key && world.door_open);
    for _ in 3..7 {
        assert_eq!(run(&tree, &mut state, &mut world), Running(Act::Walk(7)));
    }
    assert_eq!(run(&tree, &mut state, &mut world), Success);
    assert_eq!(world.at, 7);
}

#[test]
fn a_goal_achieved_by_others_ends_the_subgoals_below_it() {
    let tree = tree::<8>();
    let mut state = BtState::new(&tree);
    let mut world = World::new(7);
    assert_eq!(run(&tree, &mut state, &mut world), Running(Act::Walk(2)));
    world.door_open = true;
    assert_eq!(run(&tree, &mut state, &mut world), Running(Act::Walk(7)));
    assert_eq!(
        state.describe().to_string().split(" > ").next(),
        Some("goals {stack: [Reach(7)]}")
    );
    assert!(!world.has_key);
}

#[test]
fn a_failed_subgoal_lets_the_asker_try_another_way() {
    let tree = tree::<8>();
    let mut state = BtState::new(&tree);
    let mut world = World::new(7);
    world.key_at = None; // No key: the door cannot be opened.
    world.can_climb = true;
    assert_eq!(run(&tree, &mut state, &mut world), Running(Act::Walk(7)));
    assert!(world.climbed);
}

#[test]
fn a_failed_subgoal_is_not_asked_for_again_while_its_asker_stands() {
    let tree = tree::<8>();
    let mut state = BtState::new(&tree);
    let mut world = World::new(7);
    world.key_at = None;
    world.can_climb = true;
    world.climb_left = 2;
    assert_eq!(run(&tree, &mut state, &mut world), Running(Act::Climb));
    // Evaluate rescans Reach(7) from its first way; the door is not tried again.
    assert_eq!(run(&tree, &mut state, &mut world), Running(Act::Climb));
    assert_eq!(
        state.describe().to_string().split(" > ").next(),
        Some("goals {stack: [Reach(7), Climb], failed: [OpenDoor]}")
    );
    assert_eq!(run(&tree, &mut state, &mut world), Running(Act::Walk(7)));
    assert_eq!(world.door_tries, 1);
}

#[test]
fn a_cycle_falls_through_to_another_way() {
    let tree = tree::<8>();
    let mut state = BtState::new(&tree);
    let mut world = World::new(7);
    world.key_at = Some(9); // Behind the door it opens.
    world.can_climb = true;
    // GetKey -> Reach(9) -> OpenDoor is a cycle; Reach(9) climbs instead.
    assert_eq!(run(&tree, &mut state, &mut world), Running(Act::Walk(9)));
    assert!(world.climbed);
}

#[test]
fn a_cycle_with_no_way_out_fails() {
    let tree = tree::<8>();
    let mut state = BtState::new(&tree);
    let mut world = World::new(7);
    world.key_at = Some(9);
    assert_eq!(run(&tree, &mut state, &mut world), Failure);
}

#[test]
fn a_chain_deeper_than_the_stack_fails() {
    let tree = tree::<3>();
    let mut state = BtState::new(&tree);
    let mut world = World::new(7);
    assert_eq!(run(&tree, &mut state, &mut world), Failure);
}

#[test]
fn a_new_root_goal_starts_over() {
    let tree = tree::<8>();
    let mut state = BtState::new(&tree);
    let mut world = World::new(7);
    assert_eq!(run(&tree, &mut state, &mut world), Running(Act::Walk(2)));
    world.target = 3;
    assert_eq!(run(&tree, &mut state, &mut world), Running(Act::Walk(3)));
    assert_eq!(
        state.describe().to_string().split(" > ").next(),
        Some("goals {stack: [Reach(3)]}")
    );
}

#[test]
fn a_failure_deep_in_the_stack_returns_through_every_goal_above_it() {
    let tree = tree::<8>();
    let mut state = BtState::new(&tree);
    let mut world = World::new(7);
    world.can_climb = true;
    // Reach(7) -> OpenDoor -> GetKey -> Reach(2): walking to the key.
    assert_eq!(run(&tree, &mut state, &mut world), Running(Act::Walk(2)));
    // The key is taken. GetKey fails, so OpenDoor fails, so Reach(7) climbs.
    world.key_at = None;
    assert_eq!(run(&tree, &mut state, &mut world), Running(Act::Walk(7)));
    assert!(world.climbed && !world.door_open);
    assert_eq!(
        state.describe().to_string().split(" > ").next(),
        Some("goals {stack: [Reach(7)], failed: [OpenDoor]}")
    );
}
