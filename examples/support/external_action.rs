use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use flatbt::{BtAction, CancelOnDrop};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RequestId(u64);

/// A planning computation advanced by the worker between BT updates.
pub struct PlanningWork {
    pub request: RequestId,
    pub name: &'static str,
    pub remaining_frames: u32,
    cancellation: Arc<AtomicBool>,
}

/// Request-specific cancellation access without borrowing or locking blackboard.
pub struct RequestHandle {
    request: RequestId,
    cancellation: Arc<AtomicBool>,
}

#[derive(Default)]
pub struct Agent {
    /// Priority most recently observed by the application.
    pub urgent: bool,
    /// Internal planning work, separate from gameplay state.
    pub planning: Option<PlanningWork>,
    next_request: u64,
}

impl Agent {
    fn start_planning(&mut self, name: &'static str, frames: u32) -> Option<RequestHandle> {
        let request = RequestId(self.next_request);
        self.next_request = self.next_request.checked_add(1)?;
        // One allocation per request. Pooled/generational handles can avoid it.
        let cancellation = Arc::new(AtomicBool::new(false));
        self.planning = Some(PlanningWork {
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

    /// Applies cancellation and advances planning without running the BT.
    /// No gameplay object is moved or otherwise changed.
    /// Keeps terminal status until the BT observes it.
    pub fn advance_planning(&mut self) -> Option<RequestId> {
        let planning = self.planning.as_mut()?;
        if planning.cancellation.load(Ordering::Relaxed) {
            self.planning = None;
            return None;
        }
        if planning.remaining_frames == 0 {
            return None;
        }
        planning.remaining_frames -= 1;
        (planning.remaining_frames == 0).then_some(planning.request)
    }

    pub fn is_current(&self, request: RequestId) -> bool {
        self.planning.as_ref().is_some_and(|m| m.request == request)
    }

    fn remove_planning(&mut self, request: RequestId) {
        if self.is_current(request) {
            self.planning = None;
        }
    }
}

/// Submits a planning job and observes its answer; its handle owns cancellation.
pub struct PlanExternally {
    pub name: &'static str,
    pub frames: u32,
}

impl BtAction<Agent> for PlanExternally {
    type State = CancelOnDrop<RequestHandle>;

    fn start(&self, bb: &mut Agent, _: ()) -> Option<Self::State> {
        let request = bb.start_planning(self.name, self.frames)?;
        Some(CancelOnDrop::new(request, |request| {
            request.cancellation.store(true, Ordering::Relaxed);
        }))
    }

    /// The worker advances internal planning. No gameplay act is proposed;
    /// preparing an answer is separate from applying an act to the world.
    fn tick(&self, _: &mut Self::State, _: &mut Agent, _: ()) {}

    fn is_in_progress(&self, state: &Self::State, bb: &Agent, _: ()) -> bool {
        bb.planning
            .as_ref()
            .is_some_and(|m| m.request == state.request && m.remaining_frames > 0)
    }

    fn complete(&self, state: &mut Self::State, bb: &mut Agent, _: ()) -> bool {
        let finished = bb
            .planning
            .as_ref()
            .is_some_and(|m| m.request == state.request && m.remaining_frames == 0);
        bb.remove_planning(state.request);
        state.disarm();
        finished
    }
}
