//! Work that may take longer than a frame, as a [`Future`] a tree can wait on.
//!
//! [`Runner::run`] starts work and hands back a [`Promise`] of its answer.
//! How the work runs is the runner's business, not the asker's: at once
//! ([`Runner::Inline`]) or on Bevy's async compute pool
//! ([`Runner::Background`]). A tree waits on either the same way, with
//! [`await_future`](flatbt::await_future):
//!
//! ```
//! use flatbt_bevy::prelude::*;
//!
//! struct Mind { runner: Runner, from: u32 }
//!
//! fn plan(from: u32, to: u32) -> Option<u32> {
//!     (to >= from).then(|| to - from)
//! }
//!
//! let tree = scope! {
//!     let goal: u32 = |_: &mut Mind| 7;
//!     let steps: u32;
//!     sequence {
//!         await_future(
//!             |mind: &mut Mind, goal: &u32| {
//!                 let (from, to) = (mind.from, *goal);
//!                 Some(mind.runner.run(move || plan(from, to)))
//!             },
//!             |_: &Mind| "thinking",
//!         )
//!         .with(goal, out steps);
//!         leaf_with(|_: &mut Mind, steps: &u32| NodeResult::Running(if *steps == 5 { "walking" } else { "lost" }))
//!             .with(steps);
//!     }
//! };
//! let mut state = BtState::new(&tree);
//! // Inline: answered at once and passed on in the same update. With
//! // `Runner::Background` the tree reports "thinking" until the plan lands.
//! let mut mind = Mind { runner: Runner::Inline, from: 2 };
//! assert_eq!(update(&tree, &mut state, &mut mind, EntryMode::Resume), NodeResult::Running("walking"));
//! ```
//!
//! Dropping a promise drops its work's answer; a background task is
//! cancelled. A tree drops it when it leaves the branch that waits.

use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll};

use bevy_tasks::{AsyncComputeTaskPool, Task};

/// How work is run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Runner {
    /// At once, in the frame that asked: the answer is there on the first
    /// poll. Deterministic.
    #[default]
    Inline,
    /// On Bevy's [`AsyncComputeTaskPool`]: the answer lands in whichever frame
    /// the work finishes, and nothing waits for it. The pool must be set up,
    /// as Bevy's `TaskPoolPlugin` does.
    Background,
}

impl Runner {
    /// Runs `work`, promising its answer.
    pub fn run<T: Send + 'static>(self, work: impl FnOnce() -> T + Send + 'static) -> Promise<T> {
        Promise(match self {
            Runner::Inline => State::Ready(Some(work())),
            Runner::Background => {
                State::Running(AsyncComputeTaskPool::get().spawn(async move { work() }))
            }
        })
    }
}

/// An answer [`Runner::run`] promised: a [`Future`] that is ready at once, or
/// when background work finishes.
pub struct Promise<T>(State<T>);

enum State<T> {
    /// The answer, until it is taken.
    Ready(Option<T>),
    Running(Task<T>),
}

impl<T> Promise<T> {
    /// An answer known without any work.
    pub fn ready(value: T) -> Self {
        Self(State::Ready(Some(value)))
    }
}

impl<T: Unpin> Future for Promise<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<T> {
        match &mut self.get_mut().0 {
            State::Ready(value) => match value.take() {
                Some(value) => Poll::Ready(value),
                None => Poll::Pending,
            },
            State::Running(task) => Pin::new(task).poll(cx),
        }
    }
}

impl<T> core::fmt::Debug for Promise<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let state = match &self.0 {
            State::Ready(None) => "taken",
            State::Ready(Some(_)) => "ready",
            State::Running(_) => "running",
        };
        f.debug_tuple("Promise").field(&state).finish()
    }
}
