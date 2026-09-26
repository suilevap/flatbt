//! Authoring APIs for Bevy trees.
//!
//! FlatBT's whole prelude plus the Bevy types. The constructors are FlatBT's
//! own and need no Bevy-specific replacement.

pub use flatbt::prelude::*;

pub use crate::{
    Behavior, BehaviorCommands, BehaviorNode, BehaviorPlugin, BehaviorSystems, BehaviorTree,
    DebugBehavior, Tick, TickAt, TreeBuilder, act_every, evaluate_every,
};
