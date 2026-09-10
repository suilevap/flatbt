use std::ops::{Deref, DerefMut};

/// State-owned cancellation. Implementations own their token/sender; no context
/// is supplied. Cancellation may only enqueue a request. Must not panic.
pub trait BtCancel {
    fn cancel(&mut self);
}

/// Calls cancellation on Drop unless disarmed.
/// Construct with `new(value, cancel_fn)` or `from(value)` for [`BtCancel`] values.
/// Store in action state to cancel on preemption, rejection, reset, or Drop.
/// Normal completion also drops state: disarm after handling the outcome when
/// cancellation is no longer needed. The value's own Drop still runs.
///
/// Stores the value and an optional function pointer inline, without allocation.
/// Callbacks cannot capture variables; keep cancellation data in the value.
/// Cancellation may call the function indirectly. Deref/DerefMut expose the value.
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

    /// Suppresses cancellation; keeps the value and its ordinary Drop.
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
