//! The optional node catalog and scope DSL, driven by a Bevy context.

use bevy_ecs::prelude::*;
use bevy_ecs::query::QueryData;
use flatbt_bevy::prelude::*;
use flatbt_nodes::{BtAction, action, choose};

#[derive(Component)]
struct Ammo(u32);

#[derive(QueryData)]
#[query_data(mutable)]
struct Guard {
    ammo: &'static mut Ammo,
}

impl BehaviorContext for Guard {
    type Agent = Self;
    type Param = ();
}

struct Reload;

impl<C: BehaviorContext> BtAction<Bt<'_, '_, '_, '_, '_, C>> for Reload {
    type State = u32;

    fn start(&self, _: &mut Bt<'_, '_, '_, '_, '_, C>, _: ()) -> Option<u32> {
        Some(0)
    }

    fn is_in_progress(&self, state: &u32, _: &Bt<'_, '_, '_, '_, '_, C>, _: ()) -> bool {
        *state < 2
    }

    fn tick(&self, state: &mut u32, _: &mut Bt<'_, '_, '_, '_, '_, C>, _: ()) {
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
        choose!(|bt: &Bt<Guard>| match bt.ammo.0 {
            0 => action(Reload),
            _ => action(Reload),
        })
    });
}

// --- scope! probe -----------------------------------------------------------

struct Aim;

impl<C: BehaviorContext> flatbt_bevy::prelude::BtNode<Bt<'_, '_, '_, '_, '_, C>, &u32> for Aim {
    type State = ();

    fn update(
        &self,
        _: &mut (),
        _: &mut Bt<'_, '_, '_, '_, '_, C>,
        _: &u32,
        _: EntryMode,
    ) -> NodeResult {
        NodeResult::Success
    }
}

fn current_ammo(bt: &mut Bt<Guard>) -> u32 {
    bt.ammo.0
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
            let ammo: u32 = |bt: &mut Bt<Guard>| bt.ammo.0;
            sequence {
                Aim.with(ammo);
            }
        }
    });
}
