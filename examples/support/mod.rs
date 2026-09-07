use flatbt::{BtControl, ControlOp};

/// Repeats one child a fixed number of times, stopping at the first failure.
/// This is an application-defined policy using only the public API.
pub struct Repeat(pub usize);

impl<C> BtControl<C> for Repeat {
    type State = usize;

    fn begin(
        &self,
        _: &mut usize,
        _: &mut C,
        _active_child_index: Option<usize>,
        child_count: usize,
    ) -> ControlOp {
        if child_count != 1 {
            return ControlOp::error("Repeat expects exactly one child");
        }
        if self.0 == 0 {
            ControlOp::Success
        } else {
            ControlOp::RunChild(0)
        }
    }

    fn child_succeeded(
        &self,
        completed: &mut usize,
        _: &mut C,
        _completed_child_index: usize,
        _child_count: usize,
    ) -> ControlOp {
        *completed += 1;
        if *completed < self.0 {
            ControlOp::RunChild(0)
        } else {
            ControlOp::Success
        }
    }

    fn child_failed(
        &self,
        _: &mut usize,
        _: &mut C,
        _completed_child_index: usize,
        _child_count: usize,
    ) -> ControlOp {
        ControlOp::Failure
    }
}
