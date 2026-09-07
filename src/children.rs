use crate::{BtNode, NodeResult};

/// Static dispatch over child definitions. Implemented for tuples of arity 0–32.
pub trait BtChildren<C> {
    const LEN: usize;

    /// Runs the selected child.
    ///
    /// # Panics
    /// Tuple implementations panic if `index >= Self::LEN`.
    fn run_child(&self, index: usize, ctx: &mut C) -> NodeResult;
}

impl<C> BtChildren<C> for () {
    const LEN: usize = 0;

    fn run_child(&self, index: usize, _: &mut C) -> NodeResult {
        panic!("child index {index} out of bounds for empty children")
    }
}

// Generate every prefix once. Each arm calls its concrete child's BtNode impl;
// neither node definitions nor policies are converted to trait objects.
macro_rules! tuple_children {
    (@impl $($index:tt $node:ident),+) => {
        impl<C, $($node: BtNode<C>),+> BtChildren<C> for ($($node,)+) {
            const LEN: usize = [$(stringify!($node)),+].len();

            fn run_child(&self, index: usize, ctx: &mut C) -> NodeResult {
                match index {
                    $($index => self.$index.update(ctx),)+
                    _ => panic!("child index {index} out of bounds for {} children", Self::LEN),
                }
            }
        }
    };
    (@prefix [$($done_index:tt $done_node:ident,)*] $index:tt $node:ident $(, $tail_index:tt $tail_node:ident)*) => {
        tuple_children!(@impl $($done_index $done_node,)* $index $node);
        tuple_children!(@prefix [$($done_index $done_node,)* $index $node,] $($tail_index $tail_node),*);
    };
    (@prefix [$($done:tt)*]) => {};
}

tuple_children!(@prefix []
    0 N0, 1 N1, 2 N2, 3 N3, 4 N4, 5 N5, 6 N6, 7 N7,
    8 N8, 9 N9, 10 N10, 11 N11, 12 N12, 13 N13, 14 N14, 15 N15,
    16 N16, 17 N17, 18 N18, 19 N19, 20 N20, 21 N21, 22 N22, 23 N23,
    24 N24, 25 N25, 26 N26, 27 N27, 28 N28, 29 N29, 30 N30, 31 N31
);
