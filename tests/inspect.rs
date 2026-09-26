use flatbt::inspect::{Inspector, NodeInfo, describe};
use flatbt::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Act {
    Walk(u32),
    Wait,
}

struct World {
    enemy: Option<u32>,
    at: u32,
}

fn sees_enemy(world: &World) -> bool {
    world.enemy.is_some()
}

struct Walk;

impl BtAction<World, Act, &u32> for Walk {
    type State = ();

    fn start(&self, _: &mut World, _: &u32) -> Option<()> {
        Some(())
    }

    fn is_in_progress(&self, _: &(), world: &World, target: &u32) -> bool {
        world.at != *target
    }

    fn tick(&self, _: &mut (), _: &mut World, target: &u32) -> Act {
        Act::Walk(*target)
    }
}

fn tree() -> impl BtNode<World, Act> {
    select((
        scope! {
            let target: u32 = |world: &mut World| world.enemy.unwrap_or_default();
            let unused: u32;
            sequence {
                check(sees_enemy);
                action(Walk).with(target);
            }
        }
        .named("chase"),
        leaf(|_: &mut World| NodeResult::Running(Act::Wait)),
    ))
}

#[test]
fn a_tree_that_is_not_running_says_so() {
    let tree = tree();
    let state = BtState::new(&tree);
    assert_eq!(state.describe().to_string(), "not running");
    assert_eq!(format!("{:#}", state.describe()), "not running");
}

#[test]
fn the_running_path_is_one_line_with_names_from_code() {
    let tree = tree();
    let mut world = World {
        enemy: Some(5),
        at: 0,
    };
    let mut state = BtState::new(&tree);
    assert_eq!(
        update(&tree, &mut state, &mut world, EntryMode::Evaluate).act(),
        Some(Act::Walk(5))
    );
    assert_eq!(
        state.describe().to_string(),
        "select > chase (scope) {target: 5, unused: unset} > seq > Walk (action)"
    );
}

#[test]
fn the_alternate_form_writes_one_node_per_line() {
    let tree = tree();
    let mut world = World {
        enemy: Some(5),
        at: 0,
    };
    let mut state = BtState::new(&tree);
    let _ = update(&tree, &mut state, &mut world, EntryMode::Evaluate);
    assert_eq!(
        format!("{:#}", state.describe()),
        "select\n\
         \x20 chase (scope) {target: 5, unused: unset}\n\
         \x20   seq\n\
         \x20     Walk (action)"
    );
}

#[test]
fn inactive_nodes_are_marked_when_included() {
    let tree = tree();
    let mut state = BtState::new(&tree);
    let mut world = World { enemy: None, at: 0 };
    assert_eq!(
        update(&tree, &mut state, &mut world, EntryMode::Evaluate).act(),
        Some(Act::Wait)
    );
    assert_eq!(state.describe().to_string(), "select > leaf");
    assert_eq!(
        format!("{:#}", state.describe().with_inactive()),
        "* select\n\
         \x20 - chase (scope)\n\
         \x20   - target (compute)\n\
         \x20   - seq\n\
         \x20     - sees_enemy (check)\n\
         \x20     - Walk (action)\n\
         \x20 * leaf"
    );
}

#[test]
fn locals_without_debug_are_elided() {
    struct Secret;
    let tree = scope! {
        let secret: Secret = |_: &mut ()| Secret;
        sequence {
            leaf(|_: &mut ()| NodeResult::RUNNING);
        }
    };
    let mut state = BtState::new(&tree);
    let _ = update(&tree, &mut state, &mut (), EntryMode::Evaluate);
    assert_eq!(
        state.describe().to_string(),
        "scope {secret: ..} > seq > leaf"
    );
}

#[test]
fn choose_labels_each_arm_with_its_pattern() {
    let tree = choose!(|ammo: &u32| match *ammo {
        0 => leaf(|_: &mut u32| NodeResult::RUNNING).named("reload"),
        n if n > 3 => leaf(|_: &mut u32| NodeResult::RUNNING),
        _ => leaf(|_: &mut u32| NodeResult::RUNNING),
    });
    let mut state = BtState::new(&tree);
    let _ = update(&tree, &mut state, &mut 0, EntryMode::Evaluate);
    assert_eq!(state.describe().to_string(), "choose > 0 => reload (leaf)");
    let _ = update(&tree, &mut state, &mut 5, EntryMode::Evaluate);
    assert_eq!(state.describe().to_string(), "choose > n if n > 3 => leaf");
}

#[test]
fn policies_report_their_progress() {
    let tree = repeat(
        3,
        leaf(|n: &mut u32| {
            *n += 1;
            if n.is_multiple_of(2) {
                NodeResult::Success
            } else {
                NodeResult::RUNNING
            }
        }),
    );
    let mut state = BtState::new(&tree);
    let mut n = 0;
    let _ = update(&tree, &mut state, &mut n, EntryMode::Resume);
    let _ = update(&tree, &mut state, &mut n, EntryMode::Resume);
    assert_eq!(
        state.describe().to_string(),
        "repeat {times: 3, done: 1} > leaf"
    );
}

#[test]
fn a_slot_is_described_like_a_state() {
    let tree = invert(leaf(|_: &mut ()| NodeResult::RUNNING));
    let mut slot = None;
    let _ = update_slot(&tree, &mut slot, &mut (), EntryMode::Evaluate);
    assert_eq!(
        describe::<(), (), _>(&tree, slot.as_ref()).to_string(),
        "invert > leaf"
    );
}

#[test]
fn a_custom_inspector_receives_nodes_fields_and_nesting() {
    #[derive(Default)]
    struct Events(Vec<String>);

    impl Inspector for Events {
        fn enter(&mut self, node: NodeInfo<'_>) -> bool {
            self.0.push(format!("enter {} {}", node.kind, node.active));
            true
        }

        fn field(&mut self, name: &str, value: &dyn std::fmt::Debug) {
            self.0.push(format!("{name} = {value:?}"));
        }

        fn exit(&mut self) {
            self.0.push("exit".into());
        }
    }

    let tree = retry(2, check(|_: &()| true));
    let state: BtState<_, _> = BtState::new(&tree);
    let mut events = Events::default();
    state.inspect(&mut events);
    assert_eq!(
        events.0,
        [
            "enter retry false",
            "attempts = 2",
            "enter check false",
            "exit",
            "exit"
        ]
    );
}
