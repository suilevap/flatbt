use core::ops::{Deref, DerefMut};

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
pub struct CancelOnDrop<Inner> {
    value: Inner,
    cancel: Option<fn(&mut Inner)>,
}

impl<Inner> CancelOnDrop<Inner> {
    pub fn new(value: Inner, cancel: fn(&mut Inner)) -> Self {
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

impl<Inner: BtCancel> From<Inner> for CancelOnDrop<Inner> {
    fn from(value: Inner) -> Self {
        Self::new(value, Inner::cancel)
    }
}

impl<Inner> Deref for CancelOnDrop<Inner> {
    type Target = Inner;

    fn deref(&self) -> &Inner {
        &self.value
    }
}

impl<Inner> DerefMut for CancelOnDrop<Inner> {
    fn deref_mut(&mut self) -> &mut Inner {
        &mut self.value
    }
}

impl<Inner> Drop for CancelOnDrop<Inner> {
    fn drop(&mut self) {
        if let Some(cancel) = self.cancel {
            cancel(&mut self.value);
        }
    }
}
