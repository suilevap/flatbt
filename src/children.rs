use crate::{BtNode, EntryMode, NodeResult};

/// Static dispatch over child definitions and their corresponding state variants.
/// Implemented through the build-time FLATBT_MAX_CHILDREN limit (default 32).
/// At most one child stays active between calls.
pub trait BtChildren<C> {
    type State: Default + Send + 'static;
    const LEN: usize;

    /// Reads the active selection from the child state itself.
    fn active_child_index(&self, state: &Self::State) -> Option<usize>;

    /// Updates the active child in place or evaluates another child in local state.
    /// A terminal candidate preserves the old selection; a Running candidate
    /// replaces it. Completion of the active child clears the selection.
    /// Invalid indices report an error and return Failure without modifying state.
    fn run_child(
        &self,
        state: &mut Self::State,
        child_index: usize,
        ctx: &mut C,
        mode: EntryMode,
    ) -> NodeResult;
}

impl<C> BtChildren<C> for () {
    type State = ();
    const LEN: usize = 0;

    fn active_child_index(&self, _: &()) -> Option<usize> {
        None
    }

    fn run_child(&self, _: &mut (), child_index: usize, _: &mut C, _: EntryMode) -> NodeResult {
        NodeResult::error(format_args!(
            "child index {child_index} out of bounds for empty children"
        ))
    }
}

// Generate each tuple's state enum and concrete child dispatch together.
macro_rules! tuple_children {
    (@generate_impl $state:ident; $($index:tt $node:ident $variant:ident),+) => {
        /// State of at most one child in a statically composed tuple.
        /// The variant identifies the child; no separate saved index is needed.
        #[derive(Default)]
        pub enum $state<$($node),+> {
            #[default]
            Empty,
            $($variant($node),)+
        }

        impl<C, $($node: BtNode<C>),+> BtChildren<C> for ($($node,)+) {
            type State = $state<$($node::State),+>;
            const LEN: usize = [$(stringify!($node)),+].len();

            fn active_child_index(&self, state: &Self::State) -> Option<usize> {
                match state {
                    $state::Empty => None,
                    $($state::$variant(_) => Some($index),)+
                }
            }

            fn run_child(&self, state: &mut Self::State, child_index: usize, ctx: &mut C, mode: EntryMode) -> NodeResult {
                match child_index {
                    $($index => {
                        if let $state::$variant(active) = state {
                            let result = self.$index.update(active, ctx, mode);
                            if result != NodeResult::Running {
                                *state = $state::Empty;
                            }
                            result
                        } else {
                            // Preserve the old variant until this candidate is selected.
                            let mut candidate = $node::State::default();
                            let result = self.$index.update(&mut candidate, ctx, EntryMode::Evaluate);
                            if result == NodeResult::Running {
                                *state = $state::$variant(candidate);
                            }
                            result
                        }
                    },)+
                    _ => NodeResult::error(format_args!("child index {child_index} out of bounds for {} children", Self::LEN)),
                }
            }
        }
    };
    (@generate_prefix [$($done_index:tt $done_node:ident $done_variant:ident,)*] $state:ident $index:tt $node:ident $variant:ident $(, $tail_state:ident $tail_index:tt $tail_node:ident $tail_variant:ident)*) => {
        tuple_children!(@generate_impl $state; $($done_index $done_node $done_variant,)* $index $node $variant);
        tuple_children!(@generate_prefix [$($done_index $done_node $done_variant,)* $index $node $variant,] $($tail_state $tail_index $tail_node $tail_variant),*);
    };
    (@generate_prefix [$($done:tt)*]) => {};
}

/// Generated state enums for tuple children. Payload types are child states,
/// not node definitions. These enums are not general-purpose node combinators.
pub mod child_state {
    use super::{BtChildren, BtNode, EntryMode, NodeResult};

    include!(concat!(env!("OUT_DIR"), "/tuple_children.rs"));
}
