use crate::{BtNode, EntryMode, NodeResult, execution::run_node};

/// A tuple of statically known child state slots.
/// The wrapper supplies Default for all supported arities, including above 12.
pub struct TupleState<T>(T);

/// Static dispatch over child definitions and their corresponding state fields.
/// Implemented for tuples of arity 0–32.
pub trait BtChildren<C> {
    type State: Default + Send + 'static;
    const LEN: usize;

    /// Runs the selected child using its own typed state slot. Terminal results
    /// drop that slot; other child states are left untouched.
    /// Invalid indices report an error and return Failure.
    fn run_child(
        &self,
        state: &mut Self::State,
        child_index: usize,
        ctx: &mut C,
        mode: EntryMode,
    ) -> NodeResult;

    /// Drops the selected child's state when a new selection preempts it.
    fn reset_child(&self, state: &mut Self::State, child_index: usize);
}

impl<C> BtChildren<C> for () {
    type State = ();
    const LEN: usize = 0;

    fn run_child(&self, _: &mut (), child_index: usize, _: &mut C, _: EntryMode) -> NodeResult {
        NodeResult::error(format_args!(
            "child index {child_index} out of bounds for empty children"
        ))
    }

    fn reset_child(&self, _: &mut (), child_index: usize) {
        crate::log_error(format_args!(
            "child index {child_index} out of bounds for empty children"
        ));
    }
}

// Generate each tuple's state product and concrete child dispatch together.
macro_rules! tuple_children {
    (@generate_impl $($index:tt $node:ident),+) => {
        impl<$($node),+> Default for TupleState<($(Option<$node>,)+)> {
            fn default() -> Self {
                Self(($(Option::<$node>::None,)+))
            }
        }

        impl<C, $($node: BtNode<C>),+> BtChildren<C> for ($($node,)+) {
            type State = TupleState<($(Option<$node::State>,)+)>;
            const LEN: usize = [$(stringify!($node)),+].len();

            fn run_child(&self, state: &mut Self::State, child_index: usize, ctx: &mut C, mode: EntryMode) -> NodeResult {
                match child_index {
                    $($index => run_node(&self.$index, &mut state.0.$index, ctx, mode),)+
                    _ => NodeResult::error(format_args!("child index {child_index} out of bounds for {} children", Self::LEN)),
                }
            }

            fn reset_child(&self, state: &mut Self::State, child_index: usize) {
                match child_index {
                    $($index => state.0.$index = None,)+
                    _ => crate::log_error(format_args!("child index {child_index} out of bounds for {} children", Self::LEN)),
                }
            }
        }
    };
    (@generate_prefix [$($done_index:tt $done_node:ident,)*] $index:tt $node:ident $(, $tail_index:tt $tail_node:ident)*) => {
        tuple_children!(@generate_impl $($done_index $done_node,)* $index $node);
        tuple_children!(@generate_prefix [$($done_index $done_node,)* $index $node,] $($tail_index $tail_node),*);
    };
    (@generate_prefix [$($done:tt)*]) => {};
}

tuple_children!(@generate_prefix []
    0 N0, 1 N1, 2 N2, 3 N3, 4 N4, 5 N5, 6 N6, 7 N7,
    8 N8, 9 N9, 10 N10, 11 N11, 12 N12, 13 N13, 14 N14, 15 N15,
    16 N16, 17 N17, 18 N18, 19 N19, 20 N20, 21 N21, 22 N22, 23 N23,
    24 N24, 25 N25, 26 N26, 27 N27, 28 N28, 29 N29, 30 N30, 31 N31
);
