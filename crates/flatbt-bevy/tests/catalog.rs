//! The optional node catalog and scope DSL, driven by a Bevy context.

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_ecs::query::QueryData;
use flatbt_bevy::prelude::*;
use flatbt_nodes::{BtAction, action, choose};

#[derive(Component, PartialEq)]
struct Ammo(u32);

struct Guard {
    ammo: u32,
}

#[derive(QueryData)]
#[query_data(mutable)]
struct GuardAccess {
    ammo: &'static mut Ammo,
}

impl BehaviorContext for Guard {
    type Agent = GuardAccess;
    type Param = ();
    type Snapshot = Self;

    fn read(_: Entity, agent: &GuardAccessItem, _: &()) -> Guard {
        Guard { ammo: agent.ammo.0 }
    }

    fn write(guard: &Guard, agent: &mut GuardAccessItem) {
        agent.ammo.set_if_neq(Ammo(guard.ammo));
    }
}

struct Reload;

impl<C: BehaviorContext> BtAction<Blackboard<C>> for Reload {
    type State = u32;

    fn start(&self, _: &mut Blackboard<C>, _: ()) -> Option<u32> {
        Some(0)
    }

    fn is_in_progress(&self, state: &u32, _: &Blackboard<C>, _: ()) -> bool {
        *state < 2
    }

    fn tick(&self, state: &mut u32, _: &mut Blackboard<C>, _: ()) {
        *state += 1;
    }
}

#[test]
fn action_nodes_drive_a_bevy_context() {
    let _agent = Behavior::<Guard, _>::for_tree(|| action(Reload));
}

#[test]
fn choose_selects_on_agent_components() {
    let _tree = Behavior::<Guard, _>::for_tree(|| {
        choose!(|bb: &Blackboard<Guard>| match bb.ammo {
            0 => action(Reload),
            _ => action(Reload),
        })
    });
}

// --- scope! probe -----------------------------------------------------------

struct Aim;

impl<C: BehaviorContext> flatbt_bevy::prelude::BtNode<Blackboard<C>, &u32> for Aim {
    type State = ();

    fn update(&self, _: &mut (), _: &mut Blackboard<C>, _: &u32, _: EntryMode) -> NodeResult {
        NodeResult::Success
    }
}

fn current_ammo(bb: &mut Blackboard<Guard>) -> u32 {
    bb.ammo
}

#[test]
fn scope_initializes_locals_from_a_plain_function() {
    use flatbt_scope::scope;
    let _tree = Behavior::<Guard, _>::for_tree(|| {
        scope! {
            let ammo: u32 = current_ammo;
            sequence {
                Aim.with(ammo);
            }
        }
    });
}

#[test]
fn scope_initializes_locals_from_a_closure() {
    use flatbt_scope::scope;
    let _tree = Behavior::<Guard, _>::for_tree(|| {
        scope! {
            let ammo: u32 = |bb: &mut Blackboard<Guard>| bb.ammo;
            sequence {
                Aim.with(ammo);
            }
        }
    });
}

// --- ask into a scope local -------------------------------------------------

#[derive(Component, Clone)]
struct WantsRange;

#[derive(Component)]
struct RangeTo(u32);

struct Archer {
    range: Option<u32>,
    shots: u32,
}

#[derive(QueryData)]
#[query_data(mutable)]
struct ArcherAccess {
    range: Option<&'static RangeTo>,
    shots: &'static mut Ammo,
}

impl BehaviorContext for Archer {
    type Agent = ArcherAccess;
    type Param = ();
    type Snapshot = Self;

    fn read(_: Entity, agent: &ArcherAccessItem, _: &()) -> Archer {
        Archer {
            range: agent.range.map(|r| r.0),
            shots: agent.shots.0,
        }
    }

    fn write(archer: &Archer, agent: &mut ArcherAccessItem) {
        agent.shots.set_if_neq(Ammo(archer.shots));
    }
}

/// Reads the local the ask filled. Takes it as a value, not an `Option`: a
/// missing local fails the node before it runs.
struct Fire;

impl BtNode<Blackboard<Archer>, &u32> for Fire {
    type State = ();

    fn update(
        &self,
        _: &mut (),
        bb: &mut Blackboard<Archer>,
        range: &u32,
        _: EntryMode,
    ) -> NodeResult {
        bb.shots += *range;
        NodeResult::Success
    }
}

/// The deferred query feeding a scope local: `ask` is the initializer.
fn aim_and_fire() -> impl BehaviorNode<Archer> {
    use flatbt_scope::scope;
    scope! {
        let range: u32;
        sequence {
            ask(WantsRange, |bb: &Blackboard<Archer>| bb.range).with(out range);
            Fire.with(range);
        }
    }
}

/// Answers the request on the tick after it is made.
fn answer_range(asking: Query<Entity, With<WantsRange>>, mut commands: Commands) {
    for entity in asking.iter() {
        commands
            .entity(entity)
            .remove::<WantsRange>()
            .insert(RangeTo(7));
    }
}

#[test]
fn ask_fills_a_scope_local_and_the_nodes_after_it_read_a_value() {
    let mut app = App::new();
    app.add_plugins(BehaviorPlugin::for_tree(aim_and_fire))
        .add_systems(Update, answer_range.after(BehaviorSystems));
    let agent = app
        .world_mut()
        .spawn((Ammo(0), Behavior::for_tree(aim_and_fire)))
        .id();

    // First tick: the ask goes out and the scope waits. Nothing has fired.
    app.update();
    assert_eq!(app.world().get::<Ammo>(agent).map(|a| a.0), Some(0));

    // The answer arrived between ticks; the local takes it and `Fire` reads it.
    app.update();
    assert_eq!(app.world().get::<Ammo>(agent).map(|a| a.0), Some(7));
}
