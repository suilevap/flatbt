use core::fmt::Write;

use bevy_ecs::prelude::*;

use crate::{Behavior, BehaviorTree, TreeBuilder};

/// Keeps an agent's running path as text, for debugging.
///
/// Add it to an agent to watch; [`BehaviorPlugin`](crate::BehaviorPlugin)
/// fills it right after each tick, and agents without it cost nothing. The
/// text is current after every tick, but the component is marked changed only
/// when the path does, so a system logging `Changed<DebugBehavior>` writes one
/// line per decision rather than one per frame:
///
/// ```
/// # use bevy_ecs::prelude::*;
/// # use flatbt_bevy::prelude::*;
/// fn log_decisions(agents: Query<(Entity, &DebugBehavior), Changed<DebugBehavior>>) {
///     for (agent, debug) in agents.iter() {
///         println!("{agent}: {}", debug.path());
///     }
/// }
/// ```
///
/// An agent running more than one tree should not carry it: each tree's tick
/// would overwrite the other's path.
#[derive(Component, Default, Debug)]
pub struct DebugBehavior {
    path: String,
    id: Option<u64>,
}

impl DebugBehavior {
    /// The running path after the last tick, as
    /// [`Describe`](flatbt::inspect::Describe) writes it on one line; empty
    /// before the first.
    pub fn path(&self) -> &str {
        &self.path
    }
}

/// Refreshes [`DebugBehavior`] on agents running the tree named by `F`.
pub(crate) fn describe_behaviors<C, A, F>(
    tree: Res<BehaviorTree<C, A, F>>,
    mut agents: Query<(&Behavior<C, A, F>, &mut DebugBehavior)>,
) where
    C: Send + Sync + 'static,
    A: Send + Sync + 'static,
    F: TreeBuilder<C, A>,
{
    for (behavior, mut debug) in agents.iter_mut() {
        let id = behavior.path_id(tree.get());
        let changed = debug.id != Some(id);
        let debug_mut = debug.bypass_change_detection();
        debug_mut.id = Some(id);
        debug_mut.path.clear();
        // Writing to a `String` cannot fail.
        let _ = write!(debug_mut.path, "{}", behavior.describe(tree.get()));
        if changed {
            debug.set_changed();
        }
    }
}
