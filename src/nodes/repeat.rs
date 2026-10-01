use crate::inspect::Inspector;
use crate::{BtControl, ControlNode, ControlOp, control};

/// Runs one child `times` times in a row.
pub struct Repeat(usize);

/// Runs `child` until it has succeeded `times` times, then succeeds; fails on
/// the first failure. `times == 0` succeeds without running it.
///
/// A completed run restarts in the same update, so `times` bounds the work of
/// one update when the child completes at once.
pub fn repeat<Child>(times: usize, child: Child) -> ControlNode<Repeat, (Child,)> {
    control(Repeat(times), (child,))
}

impl<Context> BtControl<Context> for Repeat {
    /// Successes so far in this invocation.
    type State = usize;
    type Memory = ();

    fn kind(&self) -> &'static str {
        "repeat"
    }

    fn inspect(
        &self,
        done: Option<&usize>,
        _: &(),
        _: Option<usize>,
        inspector: &mut dyn Inspector,
    ) {
        inspector.field("times", &self.0);
        if let Some(done) = done {
            inspector.field("done", done);
        }
    }

    #[inline]
    fn begin(
        &self,
        done: &mut usize,
        _: &mut (),
        _: &mut Context,
        active: Option<usize>,
        _: usize,
    ) -> ControlOp {
        match active {
            Some(index) => ControlOp::RunChild(index),
            None if self.0 == 0 => ControlOp::Success,
            None => {
                *done = 0;
                ControlOp::RunChild(0)
            }
        }
    }

    #[inline]
    fn child_succeeded(
        &self,
        done: &mut usize,
        _: &mut (),
        _: &mut Context,
        _: usize,
        _: usize,
    ) -> ControlOp {
        *done += 1;
        if *done < self.0 {
            ControlOp::RunChild(0)
        } else {
            ControlOp::Success
        }
    }

    #[inline]
    fn child_failed(
        &self,
        _: &mut usize,
        _: &mut (),
        _: &mut Context,
        _: usize,
        _: usize,
    ) -> ControlOp {
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
pub fn retry<Child>(attempts: usize, child: Child) -> ControlNode<Retry, (Child,)> {
    control(Retry(attempts), (child,))
}

impl<Context> BtControl<Context> for Retry {
    /// Failures so far in this invocation.
    type State = usize;
    type Memory = ();

    fn kind(&self) -> &'static str {
        "retry"
    }

    fn inspect(
        &self,
        failed: Option<&usize>,
        _: &(),
        _: Option<usize>,
        inspector: &mut dyn Inspector,
    ) {
        inspector.field("attempts", &self.0);
        if let Some(failed) = failed {
            inspector.field("failed", failed);
        }
    }

    #[inline]
    fn begin(
        &self,
        failed: &mut usize,
        _: &mut (),
        _: &mut Context,
        active: Option<usize>,
        _: usize,
    ) -> ControlOp {
        match active {
            Some(index) => ControlOp::RunChild(index),
            None if self.0 == 0 => ControlOp::Failure,
            None => {
                *failed = 0;
                ControlOp::RunChild(0)
            }
        }
    }

    #[inline]
    fn child_succeeded(
        &self,
        _: &mut usize,
        _: &mut (),
        _: &mut Context,
        _: usize,
        _: usize,
    ) -> ControlOp {
        ControlOp::Success
    }

    #[inline]
    fn child_failed(
        &self,
        failed: &mut usize,
        _: &mut (),
        _: &mut Context,
        _: usize,
        _: usize,
    ) -> ControlOp {
        *failed += 1;
        if *failed < self.0 {
            ControlOp::RunChild(0)
        } else {
            ControlOp::Failure
        }
    }
}
