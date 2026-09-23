use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use flatbt::prelude::*;

#[derive(Debug, PartialEq)]
enum Act {
    Step,
    Walk(Walk),
}

#[derive(Debug, PartialEq)]
enum Walk {
    To(u32),
}

#[derive(Default)]
struct World {
    far: bool,
    starts: u32,
    cancelled: Arc<AtomicU32>,
    trace: Vec<&'static str>,
}

/// Runs for `updates` updates, then succeeds.
struct Stepping {
    updates: u32,
}

struct Steps {
    left: u32,
    cancelled: Arc<AtomicU32>,
}

impl Drop for Steps {
    fn drop(&mut self) {
        // Dropped mid-run: what cancellation looks like.
        if self.left > 0 {
            self.cancelled.fetch_add(1, Ordering::Relaxed);
        }
    }
}

impl BtAction<World, Act> for Stepping {
    type State = Steps;

    fn start(&self, world: &mut World, _: ()) -> Option<Steps> {
        world.starts += 1;
        world.trace.push("start");
        Some(Steps {
            left: self.updates,
            cancelled: world.cancelled.clone(),
        })
    }

    fn is_in_progress(&self, steps: &Steps, _: &World, _: ()) -> bool {
        steps.left > 0
    }

    fn tick(&self, steps: &mut Steps, _: &mut World, _: ()) -> Act {
        steps.left -= 1;
        Act::Step
    }
}

fn far(world: &World) -> bool {
    world.far
}

#[test]
fn repeat_while_succeeds_without_starting_when_the_goal_already_holds() {
    let tree = repeat_while(far, action(Stepping { updates: 1 }));
    let mut state = BtState::new(&tree);
    let mut world = World::default();

    assert_eq!(
        update(&tree, &mut state, &mut world, EntryMode::Evaluate),
        NodeResult::Success
    );
    assert_eq!(world.starts, 0);
}

#[test]
fn repeat_while_restarts_a_completed_child_in_the_same_update() {
    let tree = repeat_while(far, action(Stepping { updates: 1 }));
    let mut state = BtState::new(&tree);
    let mut world = World {
        far: true,
        ..World::default()
    };

    let result = update(&tree, &mut state, &mut world, EntryMode::Resume);
    assert_eq!(result, NodeResult::Running(Act::Step));
    // Completes, restarts and runs again: no update without an act.
    let result = update(&tree, &mut state, &mut world, EntryMode::Resume);
    assert_eq!(result, NodeResult::Running(Act::Step));
    assert_eq!(world.starts, 2);

    world.far = false;
    assert_eq!(
        update(&tree, &mut state, &mut world, EntryMode::Resume),
        NodeResult::Success
    );
    assert_eq!(world.starts, 2);
}

#[test]
fn repeat_while_ends_a_running_child_when_the_goal_holds_under_resume() {
    let tree = repeat_while(far, action(Stepping { updates: 5 }));
    let mut state = BtState::new(&tree);
    let mut world = World {
        far: true,
        ..World::default()
    };

    assert!(update(&tree, &mut state, &mut world, EntryMode::Resume).is_running());
    world.far = false;
    assert_eq!(
        update(&tree, &mut state, &mut world, EntryMode::Resume),
        NodeResult::Success
    );
    assert!(!state.is_running());
    assert_eq!(world.cancelled.load(Ordering::Relaxed), 1);
}

#[test]
fn repeat_while_succeeds_when_the_completed_child_reached_the_goal() {
    let tree = repeat_while(
        far,
        leaf(|world: &mut World| {
            world.far = false;
            NodeResult::<Act>::Success
        }),
    );
    let mut state = BtState::new(&tree);
    let mut world = World {
        far: true,
        ..World::default()
    };

    assert_eq!(
        update(&tree, &mut state, &mut world, EntryMode::Evaluate),
        NodeResult::Success
    );
}

#[test]
fn repeat_while_fails_when_its_child_cannot_keep_the_agent_busy() {
    let fails = repeat_while(far, check(|_: &World| false));
    let instant = repeat_while(far, check(|_: &World| true));
    let mut world = World {
        far: true,
        ..World::default()
    };

    let mut state: BtState<_, _, ()> = BtState::new(&fails);
    assert_eq!(
        update(&fails, &mut state, &mut world, EntryMode::Evaluate),
        NodeResult::Failure
    );
    let mut state: BtState<_, _, ()> = BtState::new(&instant);
    assert_eq!(
        update(&instant, &mut state, &mut world, EntryMode::Evaluate),
        NodeResult::Failure
    );
}

#[test]
fn repeat_while_fails_when_a_restarted_child_completes_at_once() {
    // Runs once, then completes at once on every later start.
    let tree = repeat_while(
        far,
        leaf(|world: &mut World| {
            world.starts += 1;
            if world.starts == 1 {
                NodeResult::Running(Act::Step)
            } else {
                NodeResult::Success
            }
        }),
    );
    let mut state = BtState::new(&tree);
    let mut world = World {
        far: true,
        ..World::default()
    };

    assert!(update(&tree, &mut state, &mut world, EntryMode::Resume).is_running());
    // The leaf succeeds after running, restarts, and succeeds again at once.
    assert_eq!(
        update(&tree, &mut state, &mut world, EntryMode::Resume),
        NodeResult::Failure
    );
    assert_eq!(world.starts, 3);
}

#[test]
fn repeat_while_is_a_prerequisite_for_the_next_node() {
    let tree = seq((
        repeat_while(far, action(Stepping { updates: 1 })),
        leaf(|world: &mut World| {
            world.trace.push("interact");
            NodeResult::Success
        }),
    ));
    let mut state = BtState::new(&tree);
    let mut world = World {
        far: true,
        ..World::default()
    };

    assert!(update(&tree, &mut state, &mut world, EntryMode::Resume).is_running());
    world.far = false;
    assert_eq!(
        update(&tree, &mut state, &mut world, EntryMode::Resume),
        NodeResult::Success
    );
    assert_eq!(world.trace, ["start", "interact"]);
}

#[test]
fn repeat_while_forwards_parameters() {
    let tree = scope! {
        let goal: u32 = |_: &mut u32| 2;
        sequence {
            repeat_while(
                |pos: &u32| *pos < 2,
                leaf_with(|_: &mut u32, goal: &u32| NodeResult::Running(*goal)),
            ).with(goal);
        }
    };
    let mut state = BtState::new(&tree);
    let mut pos = 0;

    assert_eq!(
        update(&tree, &mut state, &mut pos, EntryMode::Resume),
        NodeResult::Running(2)
    );
    pos = 2;
    assert_eq!(
        update(&tree, &mut state, &mut pos, EntryMode::Resume),
        NodeResult::Success
    );
}

#[test]
fn map_act_converts_the_act_and_keeps_results() {
    let walking = map_act(
        Act::Walk,
        leaf(|world: &mut World| {
            if world.far {
                NodeResult::Running(Walk::To(4))
            } else {
                NodeResult::Success
            }
        }),
    );
    let tree = seq((walking, action(Stepping { updates: 1 })));
    let mut state = BtState::new(&tree);
    let mut world = World {
        far: true,
        ..World::default()
    };

    let result = update(&tree, &mut state, &mut world, EntryMode::Evaluate);
    assert_eq!(result, NodeResult::Running(Act::Walk(Walk::To(4))));
    world.far = false;
    let result = update(&tree, &mut state, &mut world, EntryMode::Evaluate);
    assert_eq!(result, NodeResult::Running(Act::Step));
}

#[test]
fn action_while_reports_its_act_until_the_condition_ends() {
    let tree = action_while(far, |_: &World| Act::Step);
    let mut state = BtState::new(&tree);
    let mut world = World {
        far: true,
        ..World::default()
    };

    let result = update(&tree, &mut state, &mut world, EntryMode::Resume);
    assert_eq!(result, NodeResult::Running(Act::Step));
    world.far = false;
    assert_eq!(
        update(&tree, &mut state, &mut world, EntryMode::Resume),
        NodeResult::Success
    );
}

#[test]
fn check_with_reads_a_local_and_leaf_with_writes_one() {
    let tree = scope! {
        let limit: u32 = |_: &mut Vec<u32>| 3;
        let found: u32;
        sequence {
            check_with(|log: &Vec<u32>, limit: &u32| log.len() < *limit as usize).with(limit);
            leaf_with(|log: &mut Vec<u32>, found: &mut Option<u32>| {
                *found = Some(log.len() as u32);
                NodeResult::Success
            }).with(out found);
            leaf_with(|log: &mut Vec<u32>, found: &u32| {
                log.push(*found);
                NodeResult::Success
            }).with(found);
        }
    };
    let mut state: BtState<_, _> = BtState::new(&tree);
    let mut log = vec![9];

    assert_eq!(
        update(&tree, &mut state, &mut log, EntryMode::Evaluate),
        NodeResult::Success
    );
    assert_eq!(log, [9, 1]);
    log.extend([9, 9]);
    assert_eq!(
        update(&tree, &mut state, &mut log, EntryMode::Evaluate),
        NodeResult::Failure
    );
}

/// A target picked once per invocation, kept in a scope local, then followed.
#[derive(Default)]
struct Arena {
    at: u32,
    alive: [bool; 2],
    positions: [u32; 2],
    attacks: u32,
}

struct Approach;

impl BtAction<Arena, Act, &usize> for Approach {
    type State = ();

    fn start(&self, _: &mut Arena, _: &usize) -> Option<()> {
        Some(())
    }

    fn is_in_progress(&self, _: &(), _: &Arena, _: &usize) -> bool {
        true
    }

    fn tick(&self, _: &mut (), arena: &mut Arena, target: &usize) -> Act {
        Act::Walk(Walk::To(arena.positions[*target]))
    }
}

fn hunt() -> impl BtNode<Arena, Act> {
    scope! {
        let target: usize = |arena: &mut Arena| arena.alive.iter().position(|a| *a).unwrap_or(0);
        sequence {
            guard(
                |arena: &Arena, target: &usize| arena.alive[*target],
                seq((
                    repeat_while(
                        |arena: &Arena, target: &usize| arena.at < arena.positions[*target],
                        action(Approach),
                    ),
                    leaf(|arena: &mut Arena| {
                        arena.attacks += 1;
                        NodeResult::Running(Act::Step)
                    }),
                )),
            ).with(target);
        }
    }
}

#[test]
fn guard_and_repeat_while_follow_a_target_held_in_a_scope_local() {
    let tree = hunt();
    let mut state = BtState::new(&tree);
    let mut arena = Arena {
        alive: [false, true],
        positions: [9, 4],
        ..Arena::default()
    };

    // Target 1 was picked on entry; approach it while it is still far.
    let result = update(&tree, &mut state, &mut arena, EntryMode::Resume);
    assert_eq!(result, NodeResult::Running(Act::Walk(Walk::To(4))));

    arena.at = 4;
    let result = update(&tree, &mut state, &mut arena, EntryMode::Resume);
    assert_eq!(result, NodeResult::Running(Act::Step));
    assert_eq!(arena.attacks, 1);

    // The guard asks about the same target on every update.
    arena.alive[1] = false;
    let result = update(&tree, &mut state, &mut arena, EntryMode::Resume);
    assert_eq!(result, NodeResult::Failure);
    assert_eq!(arena.attacks, 1);
}

#[test]
fn repeat_while_reading_a_target_succeeds_at_once_when_the_target_is_reached() {
    let tree = hunt();
    let mut state = BtState::new(&tree);
    let mut arena = Arena {
        at: 4,
        alive: [false, true],
        positions: [9, 4],
        ..Arena::default()
    };

    let result = update(&tree, &mut state, &mut arena, EntryMode::Evaluate);
    assert_eq!(result, NodeResult::Running(Act::Step));
}

#[test]
fn action_while_follows_a_target_held_in_a_scope_local() {
    let tree = scope! {
        let target: usize = |arena: &mut Arena| arena.alive.iter().position(|a| *a).unwrap_or(0);
        sequence {
            guard(
                |arena: &Arena, target: &usize| arena.alive[*target],
                action_while(
                    |arena: &Arena, target: &usize| arena.at != arena.positions[*target],
                    |arena: &Arena, target: &usize| Act::Walk(Walk::To(arena.positions[*target])),
                ),
            ).with(target);
        }
    };
    let mut state = BtState::new(&tree);
    let mut arena = Arena {
        alive: [false, true],
        positions: [9, 4],
        ..Arena::default()
    };

    let result = update(&tree, &mut state, &mut arena, EntryMode::Resume);
    assert_eq!(result, NodeResult::Running(Act::Walk(Walk::To(4))));
    // The act is asked again every update, so it follows the target it was given.
    arena.positions[1] = 6;
    let result = update(&tree, &mut state, &mut arena, EntryMode::Resume);
    assert_eq!(result, NodeResult::Running(Act::Walk(Walk::To(6))));

    arena.at = 6;
    let result = update(&tree, &mut state, &mut arena, EntryMode::Resume);
    assert_eq!(result, NodeResult::Success);
}

#[test]
fn one_constructor_takes_either_shape_under_the_same_binding() {
    let tree = scope! {
        let target: usize = |_: &mut Arena| 1;
        sequence {
            // Context only: ignores the target it is handed.
            guard(
                |arena: &Arena| arena.alive.contains(&true),
                // Context and target.
                action_while(
                    |arena: &Arena, target: &usize| arena.alive[*target],
                    |_: &Arena| Act::Step,
                ),
            ).with(target);
        }
    };
    let mut state = BtState::new(&tree);
    let mut arena = Arena {
        alive: [true, true],
        ..Arena::default()
    };

    let result = update(&tree, &mut state, &mut arena, EntryMode::Resume);
    assert_eq!(result, NodeResult::Running(Act::Step));
    arena.alive[1] = false;
    let result = update(&tree, &mut state, &mut arena, EntryMode::Resume);
    assert_eq!(result, NodeResult::Success);
}
