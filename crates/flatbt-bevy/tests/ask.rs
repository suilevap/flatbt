//! Asking the world for something the blackboard does not hold yet.
//!
//! A question is an action like any other: the tree starts it, it becomes a
//! component, a system matching that component answers, and the tree waits.
//! What `ask` adds over a hand-written action is that the question is put once
//! per invocation rather than once per tick, and that the answer arrives as a
//! `scope!` local -- so the node after it takes a value, not an `Option`, and
//! the answer does not outlive the decision that wanted it.

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use flatbt_bevy::prelude::*;
use flatbt_nodes::{Request, ask};
use flatbt_scope::scope;

#[derive(Component, Default, Debug)]
struct Fighter {
    health: f32,
    /// The question and its answer, in one field. `Pending` becomes the
    /// `LookingForCover` component below; the answer comes back through the
    /// gather like any other reading of the world.
    cover: Request<f32>,
    move_to: Option<f32>,
}

/// The question, as a component. Nothing that answers it mentions `Fighter`.
#[derive(Component, Default, PartialEq, Debug)]
struct LookingForCover;

/// Where the game keeps cover.
#[derive(Component)]
struct Cover(f32);

/// The node that consumes the answer. It takes an `f32`, so it cannot run
/// without one.
struct WalkTo;

impl BtNode<Fighter, &f32> for WalkTo {
    type State = ();

    fn update(&self, _: &mut (), f: &mut Fighter, spot: &f32, _: EntryMode) -> NodeResult {
        f.move_to = Some(*spot);
        NodeResult::Success
    }
}

fn take_cover() -> impl BehaviorNode<Fighter> {
    scope! {
        let spot: f32;
        sequence {
            ask(
                |f: &mut Fighter| f.cover.ask(),
                |f: &Fighter| f.cover.answered().copied(),
            ).with(out spot);
            WalkTo.with(spot);
        }
    }
}

/// The system that answers. It matches the component the question became, and
/// runs a query no node could run for itself.
fn find_cover(
    mut asking: Query<(&Transform2d, &mut Fighter), With<LookingForCover>>,
    cover: Query<&Cover>,
) {
    let spots: Vec<f32> = cover.iter().map(|c| c.0).collect();
    for (at, mut fighter) in asking.iter_mut() {
        let nearest = spots
            .iter()
            .copied()
            .min_by(|a, b| (a - at.0).abs().total_cmp(&(b - at.0).abs()));
        let fighter = fighter.bypass_change_detection();
        match nearest {
            Some(spot) => fighter.cover.answer(spot),
            None => fighter.cover.clear(),
        }
    }
}

#[derive(Component)]
struct Transform2d(f32);

fn app() -> App {
    let mut app = App::new();
    app.add_plugins(BehaviorPlugin::for_tree(take_cover).tick_mode(|_| Tick::Resume))
        .add_plugins(ActionComponent::<_, LookingForCover>::while_(
            |f: &Fighter| f.cover.is_pending(),
        ))
        .add_systems(Update, find_cover.after(ActionSystems));
    app.world_mut().spawn(Cover(42.0));
    app
}

#[test]
fn a_question_becomes_a_component_and_a_system_answers_it() {
    let mut app = app();
    let agent = app
        .world_mut()
        .spawn((
            Transform2d(40.0),
            Fighter::default(),
            Behavior::for_tree(take_cover),
        ))
        .id();

    // Tick one: the tree asks, the bridge inserts the component, the system
    // answers into the blackboard.
    app.update();
    assert!(
        app.world().get::<LookingForCover>(agent).is_some(),
        "the question is a component while it stands"
    );
    assert_eq!(app.world().get::<Fighter>(agent).unwrap().move_to, None);

    // Tick two: the answer is in, the action completes, `WalkTo` gets an `f32`,
    // and the question component goes with the question.
    app.update();
    assert_eq!(
        app.world().get::<Fighter>(agent).unwrap().move_to,
        Some(42.0)
    );
    assert!(app.world().get::<LookingForCover>(agent).is_none());
}

/// Asking is once per invocation, not once per tick. That is the whole reason
/// it is an action: a leaf returning `Running` is re-entered on every resume,
/// so a leaf that asks would re-ask -- and with the question a component, that
/// is an archetype move twice a frame.
#[test]
fn a_waiting_question_is_not_asked_again_every_tick() {
    #[derive(Resource, Default)]
    struct Inserts(u32);

    fn count_inserts(added: Query<(), Added<LookingForCover>>, mut inserts: ResMut<Inserts>) {
        inserts.0 += added.iter().count() as u32;
    }

    let mut app = App::new();
    app.init_resource::<Inserts>()
        .add_plugins(BehaviorPlugin::for_tree(take_cover).tick_mode(|_| Tick::Resume))
        .add_plugins(ActionComponent::<_, LookingForCover>::while_(
            |f: &Fighter| f.cover.is_pending(),
        ))
        .add_systems(Update, count_inserts.after(ActionSystems));
    app.world_mut().spawn((
        Transform2d(0.0),
        Fighter::default(),
        Behavior::for_tree(take_cover),
    ));

    // Nothing answers, so the question stands for five ticks.
    for _ in 0..5 {
        app.update();
    }

    assert_eq!(
        app.world().resource::<Inserts>().0,
        1,
        "asked once, then waited"
    );
}

/// The local belongs to the invocation: leaving the branch and coming back asks
/// again rather than acting on an answer chosen for an older situation.
#[test]
fn the_answer_does_not_outlive_its_invocation() {
    fn hurt_only() -> impl BehaviorNode<Fighter> {
        seq((check(|f: &Fighter| f.health < 40.0), take_cover()))
    }

    let mut app = App::new();
    app.add_plugins(BehaviorPlugin::for_tree(hurt_only).tick_mode(|_| Tick::Resume))
        .add_plugins(ActionComponent::<_, LookingForCover>::while_(
            |f: &Fighter| f.cover.is_pending(),
        ))
        .add_systems(Update, find_cover.after(ActionSystems));
    app.world_mut().spawn(Cover(42.0));
    let agent = app
        .world_mut()
        .spawn((
            Transform2d(40.0),
            Fighter {
                health: 10.0,
                ..Fighter::default()
            },
            Behavior::for_tree(hurt_only),
        ))
        .id();

    app.update();
    app.update();
    assert_eq!(
        app.world().get::<Fighter>(agent).unwrap().move_to,
        Some(42.0)
    );

    // Heal, so the guard fails and the invocation ends; clear what it decided.
    {
        let mut fighter = app.world_mut().get_mut::<Fighter>(agent).unwrap();
        fighter.health = 100.0;
        fighter.move_to = None;
        fighter.cover.clear();
    }
    app.update();
    app.world_mut().get_mut::<Fighter>(agent).unwrap().health = 10.0;
    app.update();

    assert!(
        app.world().get::<LookingForCover>(agent).is_some(),
        "it asked again rather than reusing the old answer"
    );
}
