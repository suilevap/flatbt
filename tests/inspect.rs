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
    /// Where the walk is headed, fixed on start.
    type State = u32;

    fn start(&self, _: &mut World, target: &u32) -> Option<u32> {
        Some(*target)
    }

    fn is_in_progress(&self, to: &u32, world: &World, _: &u32) -> bool {
        world.at != *to
    }

    fn tick(&self, to: &mut u32, _: &mut World, _: &u32) -> Act {
        Act::Walk(*to)
    }

    fn inspect(&self, to: Option<&u32>, inspector: &mut dyn Inspector) {
        if let Some(to) = to {
            inspector.field("to", to);
        }
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
        "select > chase (scope) {target: 5, unused: unset} > seq > Walk (action) {to: 5}"
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
         \x20     Walk (action) {to: 5}"
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
        0 => named("reload", leaf(|_: &mut u32| NodeResult::RUNNING)),
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

#[test]
fn orders_report_their_position_and_the_children_tried() {
    struct Needs {
        hunger: f32,
        fatigue: f32,
    }
    let (score, options) = per_child!(|needs: &Needs| {
        needs.hunger => leaf(|_: &mut Needs| NodeResult::<&str>::Failure),
        needs.fatigue => leaf(|_: &mut Needs| NodeResult::Running("sleep")),
    });
    let tree = select(order_by(by_score(score), options));
    let mut state = BtState::new(&tree);
    let mut needs = Needs {
        hunger: 0.9,
        fatigue: 0.5,
    };
    let _ = update(&tree, &mut state, &mut needs, EntryMode::Evaluate);
    assert_eq!(
        state.describe().to_string(),
        "select {order: by_score, position: 1, tried: {0}} > needs.fatigue => leaf"
    );
}

#[test]
fn the_path_id_changes_with_the_path_and_not_with_fields() {
    let tree = tree();
    let mut state = BtState::new(&tree);
    let idle = state.path_id();
    let mut world = World {
        enemy: Some(5),
        at: 0,
    };
    let _ = update(&tree, &mut state, &mut world, EntryMode::Evaluate);
    let chasing = state.path_id();
    assert_ne!(chasing, idle);

    // A new target is a field, not a new path.
    world.enemy = Some(7);
    state.reset();
    let _ = update(&tree, &mut state, &mut world, EntryMode::Evaluate);
    assert!(state.describe().to_string().contains("target: 7"));
    assert_eq!(state.path_id(), chasing);

    world.enemy = None;
    state.reset();
    let _ = update(&tree, &mut state, &mut world, EntryMode::Evaluate);
    assert_ne!(state.path_id(), chasing);
    assert_ne!(state.path_id(), idle);
}
