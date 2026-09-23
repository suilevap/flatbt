use crate::{BtNode, EntryMode, NodeResult};

/// Static tuple dispatch with at most one active child.
/// Generated through `FLATBT_MAX_CHILDREN` (default 32).
pub trait BtChildren<C, A = (), P = ()> {
    type State: Default + Send + 'static;
    const LEN: usize;

    /// Reads selection from the saved state variant.
    fn active_child_index(&self, state: &Self::State) -> Option<usize>;

    /// Updates the saved child in place or enters a fresh candidate with Evaluate.
    /// Terminal candidates preserve saved state; Running candidates replace it.
    /// A terminal saved child clears selection. Invalid indices log an error and
    /// return Failure without changing state.
    fn run_child(
        &self,
        state: &mut Self::State,
        child_index: usize,
        ctx: &mut C,
        params: P,
        mode: EntryMode,
    ) -> NodeResult<A>;
}

impl<C, A, P> BtChildren<C, A, P> for () {
    type State = ();
    const LEN: usize = 0;

    fn active_child_index(&self, _: &()) -> Option<usize> {
        None
    }

    fn run_child(
        &self,
        _: &mut (),
        child_index: usize,
        _: &mut C,
        _: P,
        _: EntryMode,
    ) -> NodeResult<A> {
        NodeResult::error(format_args!(
            "child index {child_index} out of bounds for empty children"
        ))
    }
}

macro_rules! tuple_children {
    (@generate_impl $state:ident; $($index:tt $node:ident $variant:ident),+) => {
        /// One active child state; the variant encodes its index.
        #[derive(Default)]
        pub enum $state<$($node),+> {
            #[default]
            Empty,
            $($variant($node),)+
        }

        impl<C, A, P, $($node: BtNode<C, A, P>),+> BtChildren<C, A, P> for ($($node,)+) {
            type State = $state<$($node::State),+>;
            const LEN: usize = [$(stringify!($node)),+].len();

            #[inline(always)]
            fn active_child_index(&self, state: &Self::State) -> Option<usize> {
                match state {
                    $state::Empty => None,
                    $($state::$variant(_) => Some($index),)+
                }
            }

            #[inline(always)]
            fn run_child(&self, state: &mut Self::State, child_index: usize, ctx: &mut C, params: P, mode: EntryMode) -> NodeResult<A> {
                match child_index {
                    $($index => {
                        if let $state::$variant(active) = state {
                            let result = self.$index.update(active, ctx, params, mode);
                            if !result.is_running() {
                                *state = $state::Empty;
                            }
                            result
                        } else {
                            // Preserve the old variant until this candidate is selected.
                            let mut candidate = $node::State::default();
                            let result = self.$index.update(&mut candidate, ctx, params, EntryMode::Evaluate);
                            if result.is_running() {
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

/// Generated tuple state enums, parameterized by child state types.
pub mod child_state {
    use super::{BtChildren, BtNode, EntryMode, NodeResult};

    include!(concat!(env!("OUT_DIR"), "/tuple_children.rs"));
}
