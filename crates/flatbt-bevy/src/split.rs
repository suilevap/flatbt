use core::ops::Deref;

use bevy_ecs::prelude::*;

/// A blackboard split into what the tree may read and what it may write.
///
/// The tick takes `&mut C`, so a blackboard is mutable in full and "the tree
/// only reads the top half" is a convention. This makes it a rule: `In` is
/// reachable by [`Deref`] and nothing else, so a node can read it and cannot
/// write it. Writing goes through [`out`](Self::out), which hands out `&mut Out`
/// and nothing else.
///
/// ```
/// # use bevy_ecs::prelude::*;
/// # use flatbt_bevy::prelude::*;
/// #[derive(Default)]
/// struct Sensed {
///     health: f32,
///     ammo: u32,
/// }
///
/// #[derive(Default)]
/// struct Orders {
///     shoot: bool,
///     retreat: bool,
/// }
///
/// type Fighter = Split<Sensed, Orders>;
///
/// fn fight() -> impl BehaviorNode<Fighter> {
///     select((
///         seq((
///             // Reading the input needs no ceremony: `Deref` reaches it.
///             check(|f: &Fighter| f.ammo > 0),
///             leaf(|f: &mut Fighter| {
///                 f.out().shoot = true;
///                 NodeResult::Success
///             }),
///         )),
///         leaf(|f: &mut Fighter| {
///             f.out().retreat = true;
///             NodeResult::Success
///         }),
///     ))
/// }
/// ```
///
/// A node that tries to write the input does not compile:
///
/// ```compile_fail
/// # use bevy_ecs::prelude::*;
/// # use flatbt_bevy::prelude::*;
/// # #[derive(Default)]
/// # struct Sensed { ammo: u32 }
/// # #[derive(Default)]
/// # struct Orders { shoot: bool }
/// fn cheat() -> impl BehaviorNode<Split<Sensed, Orders>> {
///     leaf(|f: &mut Split<Sensed, Orders>| {
///         f.ammo -= 1; // no `DerefMut`, so the input cannot be written here
///         NodeResult::Success
///     })
/// }
/// ```
///
/// It costs nothing: one component, the same two-term tick query, the same
/// layout. Everything FlatBT ships composes over it unchanged -- `seq`,
/// `select`, `check`, `leaf`, `choose!`, `scope!`, `action` and a custom
/// `BtNode` all name `Split<In, Out>` the way they would name any blackboard.
///
/// # What it does not do
///
/// The gather needs `&mut In`, and a Rust type cannot tell a gather system from
/// a node, so [`sensed_mut`](Self::sensed_mut) is public. What the split buys is
/// that no node reaches the input by accident: there is no `DerefMut`, so
/// writing the read-only half means naming a method written for someone else.
///
/// Nor is it required. A game whose blackboard has no meaningful split writes a
/// plain component and keeps its field names flat; this is here for the one
/// that wants the halves enforced rather than commented.
#[derive(Component, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Split<In, Out> {
    sensed: In,
    orders: Out,
}

impl<In, Out> Split<In, Out> {
    pub fn new(sensed: In, orders: Out) -> Self {
        Self { sensed, orders }
    }

    /// The tree's only write handle.
    pub fn out(&mut self) -> &mut Out {
        &mut self.orders
    }

    /// What the tree decided, for the system that carries it out.
    pub fn orders(&self) -> &Out {
        &self.orders
    }

    /// For the gather. Deliberately not `DerefMut`: see [What it does not
    /// do](Self#what-it-does-not-do).
    pub fn sensed_mut(&mut self) -> &mut In {
        &mut self.sensed
    }

    /// Takes the decision out and leaves `Out::default()` behind, which is how
    /// a system that carries orders out also clears them.
    pub fn take_orders(&mut self) -> Out
    where
        Out: Default,
    {
        core::mem::take(&mut self.orders)
    }
}

impl<In, Out> Deref for Split<In, Out> {
    type Target = In;

    fn deref(&self) -> &In {
        &self.sensed
    }
}
