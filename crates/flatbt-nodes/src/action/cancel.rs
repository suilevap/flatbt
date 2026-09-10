use std::ops::{Deref, DerefMut};

/// Cancellation owned by an invocation's state or an external request handle.
/// Implementations own the token or sender they need; no blackboard is supplied.
/// Cancellation may only enqueue a request. Implementations should not panic.
pub trait BtCancel {
    fn cancel(&mut self);
}

/// Calls a supplied cancellation function on destruction unless disarmed.
/// Use `new(value, cancel)` to define cancellation next to resource acquisition,
/// or `CancelOnDrop::from(value)` when the value implements `BtCancel`.
/// Store this in action state to cancel on preemption, reset, or abandonment.
/// Normal completion also drops state: call `disarm` in `BtAction::complete`
/// after handling either success or failure if cancellation is no longer needed.
///
/// The value and an optional function pointer are stored inline, without
/// allocation. The function may be called indirectly when cancellation runs.
/// Callbacks cannot capture variables; keep cancellation data in the value.
/// Access to the wrapped state is provided through Deref/DerefMut.
/// Disarming suppresses cancellation, not the wrapped value's ordinary Drop.
pub struct CancelOnDrop<T> {
    value: T,
    cancel: Option<fn(&mut T)>,
}

impl<T> CancelOnDrop<T> {
    pub fn new(value: T, cancel: fn(&mut T)) -> Self {
        Self {
            value,
            cancel: Some(cancel),
        }
    }

    /// Suppresses cancellation when this owner is dropped.
    pub fn disarm(&mut self) {
        self.cancel = None;
    }
}

impl<T: BtCancel> From<T> for CancelOnDrop<T> {
    fn from(value: T) -> Self {
        Self::new(value, T::cancel)
    }
}

impl<T> Deref for CancelOnDrop<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.value
    }
}

impl<T> DerefMut for CancelOnDrop<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.value
    }
}

impl<T> Drop for CancelOnDrop<T> {
    fn drop(&mut self) {
        if let Some(cancel) = self.cancel {
            cancel(&mut self.value);
        }
    }
}
