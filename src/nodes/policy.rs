//! Small control policies over one or two children.

use crate::{BtControl, ControlNode, ControlOp, control};

/// Runs one child `times` times in a row.
pub struct Repeat(usize);

/// Runs `child` until it has succeeded `times` times, then succeeds; fails on
/// the first failure. `times == 0` succeeds without running it.
///
/// A completed run restarts in the same update, so `times` bounds the work of
/// one update when the child completes at once.
pub fn repeat<N>(times: usize, child: N) -> ControlNode<Repeat, (N,)> {
    control(Repeat(times), (child,))
}

impl<C> BtControl<C> for Repeat {
    /// Successes so far in this invocation.
    type State = usize;

    #[inline(always)]
    fn begin(&self, done: &mut usize, _: &mut C, active: Option<usize>, _: usize) -> ControlOp {
        match active {
            Some(index) => ControlOp::RunChild(index),
            None if self.0 == 0 => ControlOp::Success,
            None => {
                *done = 0;
                ControlOp::RunChild(0)
            }
        }
    }

    #[inline(always)]
    fn child_succeeded(&self, done: &mut usize, _: &mut C, _: usize, _: usize) -> ControlOp {
        *done += 1;
        if *done < self.0 {
            ControlOp::RunChild(0)
        } else {
            ControlOp::Success
        }
    }

    #[inline(always)]
    fn child_failed(&self, _: &mut usize, _: &mut C, _: usize, _: usize) -> ControlOp {
        ControlOp::Failure
    }
}

/// Runs one child until it succeeds, at most `attempts` times.
pub struct Retry(usize);

/// Runs `child` until it succeeds, at most `attempts` times, and fails when
/// every attempt failed. `attempts == 0` fails without running it.
///
/// A failed attempt restarts in the same update, so `attempts` bounds the work
/// of one update.
pub fn retry<N>(attempts: usize, child: N) -> ControlNode<Retry, (N,)> {
    control(Retry(attempts), (child,))
}

impl<C> BtControl<C> for Retry {
    /// Failures so far in this invocation.
    type State = usize;

    #[inline(always)]
    fn begin(&self, failed: &mut usize, _: &mut C, active: Option<usize>, _: usize) -> ControlOp {
        match active {
            Some(index) => ControlOp::RunChild(index),
            None if self.0 == 0 => ControlOp::Failure,
            None => {
                *failed = 0;
                ControlOp::RunChild(0)
            }
        }
    }

    #[inline(always)]
    fn child_succeeded(&self, _: &mut usize, _: &mut C, _: usize, _: usize) -> ControlOp {
        ControlOp::Success
    }

    #[inline(always)]
    fn child_failed(&self, failed: &mut usize, _: &mut C, _: usize, _: usize) -> ControlOp {
        *failed += 1;
        if *failed < self.0 {
            ControlOp::RunChild(0)
        } else {
            ControlOp::Failure
        }
    }
}

/// Runs one of two children by a condition.
pub struct IfElse<F>(F);

/// Runs `then` when `condition` holds, `otherwise` when it does not, and
/// returns that child's result without fallback.
///
/// Like [`choose!`](crate::choose) with two arms: Evaluate asks again and may
/// switch branch, dropping the running one; Resume keeps the running branch.
///
/// ```
/// use flatbt::prelude::*;
///
/// let tree = if_else(
///     |ammo: &u32| *ammo > 0,
///     leaf(|_: &mut u32| NodeResult::Running("fire")),
///     leaf(|_: &mut u32| NodeResult::Running("reload")),
/// );
/// let mut state = BtState::new(&tree);
/// assert_eq!(update(&tree, &mut state, &mut 0, EntryMode::Evaluate).act(), Some("reload"));
/// ```
pub fn if_else<F, T, E>(condition: F, then: T, otherwise: E) -> ControlNode<IfElse<F>, (T, E)> {
    control(IfElse(condition), (then, otherwise))
}

impl<C, F: Fn(&C) -> bool> BtControl<C> for IfElse<F> {
    type State = ();

    #[inline(always)]
    fn begin(&self, _: &mut (), ctx: &mut C, _: Option<usize>, _: usize) -> ControlOp {
        ControlOp::RunChild(if (self.0)(ctx) { 0 } else { 1 })
    }

    #[inline(always)]
    fn child_succeeded(&self, _: &mut (), _: &mut C, _: usize, _: usize) -> ControlOp {
        ControlOp::Success
    }

    #[inline(always)]
    fn child_failed(&self, _: &mut (), _: &mut C, _: usize, _: usize) -> ControlOp {
        ControlOp::Failure
    }
}
