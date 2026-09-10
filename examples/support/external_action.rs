use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use flatbt::{BtAction, CancelOnDrop};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RequestId(u64);

/// Movement advanced by the external system between BT updates.
pub struct Movement {
    pub request: RequestId,
    pub name: &'static str,
    pub remaining_frames: u32,
    cancellation: Arc<AtomicBool>,
}

/// Request-specific cancellation access without borrowing or locking context.
pub struct RequestHandle {
    request: RequestId,
    cancellation: Arc<AtomicBool>,
}

#[derive(Default)]
pub struct Agent {
    pub urgent: bool,
    pub movement: Option<Movement>,
    next_request: u64,
}

impl Agent {
    fn start_movement(&mut self, name: &'static str, frames: u32) -> Option<RequestHandle> {
        let request = RequestId(self.next_request);
        self.next_request = self.next_request.checked_add(1)?;
        // One allocation per request. Pooled/generational handles can avoid it.
        let cancellation = Arc::new(AtomicBool::new(false));
        self.movement = Some(Movement {
            request,
            name,
            remaining_frames: frames,
            cancellation: cancellation.clone(),
        });
        Some(RequestHandle {
            request,
            cancellation,
        })
    }

    /// Applies cancellation and advances movement without running the BT.
    /// Keeps terminal status until the BT observes it.
    pub fn advance_movement(&mut self) -> Option<RequestId> {
        let movement = self.movement.as_mut()?;
        if movement.cancellation.load(Ordering::Relaxed) {
            self.movement = None;
            return None;
        }
        if movement.remaining_frames == 0 {
            return None;
        }
        movement.remaining_frames -= 1;
        (movement.remaining_frames == 0).then_some(movement.request)
    }

    pub fn is_current(&self, request: RequestId) -> bool {
        self.movement.as_ref().is_some_and(|m| m.request == request)
    }

    fn remove_movement(&mut self, request: RequestId) {
        if self.is_current(request) {
            self.movement = None;
        }
    }
}

/// Submits and observes work; its handle owns cancellation.
pub struct MoveExternally {
    pub name: &'static str,
    pub frames: u32,
}

impl BtAction<Agent> for MoveExternally {
    type State = CancelOnDrop<RequestHandle>;

    fn start(&self, ctx: &mut Agent, _: ()) -> Option<Self::State> {
        let request = ctx.start_movement(self.name, self.frames)?;
        Some(CancelOnDrop::new(request, |request| {
            request.cancellation.store(true, Ordering::Relaxed);
        }))
    }

    fn is_in_progress(&self, state: &Self::State, ctx: &Agent, _: ()) -> bool {
        ctx.movement
            .as_ref()
            .is_some_and(|m| m.request == state.request && m.remaining_frames > 0)
    }

    fn complete(&self, state: &mut Self::State, ctx: &mut Agent, _: ()) -> bool {
        let finished = ctx
            .movement
            .as_ref()
            .is_some_and(|m| m.request == state.request && m.remaining_frames == 0);
        ctx.remove_movement(state.request);
        state.disarm();
        finished
    }
}
