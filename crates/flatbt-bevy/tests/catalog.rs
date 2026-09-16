//! The optional node catalog and scope DSL, driven by a Bevy context.

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
