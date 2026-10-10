//! Waiting on a [`Future`] from a tree.

use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll, Waker};

use super::{ActionNode, BtAction, action};
use crate::inspect::Inspector;

/// Starts work with `start` from an input, reports `waiting` until the work's
/// future answers, then writes the answer to an output and succeeds.
///
/// Parameters are `(&Input, &mut Option<T>)`, typically `scope!` locals:
/// `.with(input, out answer)`. `start` returns the future, or `None` to fail
/// without starting. The future answers `Option<T>`: `Some` is written to the
/// output, `None` fails the node.
///
/// The future lives in the node's invocation state: leaving the branch drops
/// it, which cancels whatever drops cancel (a Bevy task, for one). It must be
/// `Unpin`; pin a future that is not with `Box::pin`.
///
/// The tree polls; nothing wakes it. Each update polls once, with a no-op
/// waker, so the answer is seen on the update after it lands. Keep updating
/// the tree (`Resume` or `Evaluate`) while it waits. An answer that is ready
/// at once is taken on the update that started the work, which then goes on
/// to the next node.
///
/// ```
/// use flatbt::prelude::*;
///
/// struct Game { asked: u32 }
///
/// let tree = scope! {
///     let cell: u32 = |_: &mut Game| 7;
///     let path_len: u32;
///     sequence {
///         // Any `Future<Output = Option<T>> + Unpin`: a Bevy task, a
///         // service's promise. This one is ready at once.
///         await_future(
///             |game: &mut Game, cell: &u32| {
///                 game.asked += 1;
///                 Some(core::future::ready(Some(cell * 2)))
///             },
///             |_: &Game| "thinking",
///         )
///         .with(cell, out path_len);
///         leaf_with(|_: &mut Game, len: &u32| NodeResult::Running(if *len == 14 { "walking" } else { "lost" }))
///             .with(path_len);
///     }
/// };
/// let mut state = BtState::new(&tree);
/// let mut game = Game { asked: 0 };
/// // Ready at once: answered and passed on in the same update.
/// assert_eq!(update(&tree, &mut state, &mut game, EntryMode::Evaluate), NodeResult::Running("walking"));
/// assert_eq!(game.asked, 1);
/// ```
pub fn await_future<Start, Waiting>(
    start: Start,
    waiting: Waiting,
) -> ActionNode<AwaitFuture<Start, Waiting>> {
    action(AwaitFuture { start, waiting })
}

/// The action behind [`await_future`].
pub struct AwaitFuture<Start, Waiting> {
    start: Start,
    waiting: Waiting,
}

/// A future being waited on, and its answer once polled out of it.
pub struct Awaiting<F: Future> {
    future: F,
    answer: Option<F::Output>,
}

impl<F: Future + Unpin> Awaiting<F> {
    /// Polls once, unless the future has answered already.
    fn poll(&mut self) {
        if self.answer.is_some() {
            return;
        }
        let mut cx = Context::from_waker(Waker::noop());
        if let Poll::Ready(answer) = Pin::new(&mut self.future).poll(&mut cx) {
            self.answer = Some(answer);
        }
    }
}

impl<'a, Ctx, Act, Input, T, F, Start, Waiting> BtAction<Ctx, Act, (&'a Input, &'a mut Option<T>)>
    for AwaitFuture<Start, Waiting>
where
    Input: ?Sized,
    F: Future<Output = Option<T>> + Unpin + Send + 'static,
    T: Send + 'static,
    Start: Fn(&mut Ctx, &Input) -> Option<F>,
    Waiting: Fn(&Ctx) -> Act,
{
    type State = Awaiting<F>;

    fn start(
        &self,
        ctx: &mut Ctx,
        (input, _): (&'a Input, &'a mut Option<T>),
    ) -> Option<Awaiting<F>> {
        let mut awaiting = Awaiting {
            future: (self.start)(ctx, input)?,
            answer: None,
        };
        awaiting.poll();
        Some(awaiting)
    }

    fn is_in_progress(
        &self,
        awaiting: &Awaiting<F>,
        _: &Ctx,
        _: (&'a Input, &'a mut Option<T>),
    ) -> bool {
        awaiting.answer.is_none()
    }

    fn tick(
        &self,
        awaiting: &mut Awaiting<F>,
        ctx: &mut Ctx,
        _: (&'a Input, &'a mut Option<T>),
    ) -> Act {
        awaiting.poll();
        (self.waiting)(ctx)
    }

    fn complete(
        &self,
        awaiting: &mut Awaiting<F>,
        _: &mut Ctx,
        (_, out): (&'a Input, &'a mut Option<T>),
    ) -> bool {
        match awaiting.answer.take().flatten() {
            Some(value) => {
                *out = Some(value);
                true
            }
            None => false,
        }
    }

    fn inspect(&self, awaiting: Option<&Awaiting<F>>, inspector: &mut dyn Inspector) {
        if let Some(awaiting) = awaiting {
            inspector.field("answered", &awaiting.answer.is_some());
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use core::future::{Future, ready};
    use core::pin::Pin;
    use core::task::{Context, Poll};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    use crate::prelude::*;

    /// Answers `answer` after `polls` pending polls; counts drops in `dropped`.
    struct Slow {
        polls: u32,
        answer: Option<u32>,
        dropped: Arc<AtomicU32>,
    }

    impl Future for Slow {
        type Output = Option<u32>;
        fn poll(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<u32>> {
            if self.polls > 0 {
                self.polls -= 1;
                Poll::Pending
            } else {
                Poll::Ready(self.answer)
            }
        }
    }

    impl Drop for Slow {
        fn drop(&mut self) {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[derive(Default)]
    struct World {
        polls: u32,
        answer: Option<u32>,
        dropped: Arc<AtomicU32>,
        got: Option<u32>,
        go: bool,
    }

    /// Doubles the scope's input, through a future the world configures.
    fn tree() -> impl BtNode<World, &'static str> {
        let wait = scope! {
            let input: u32 = |_: &mut World| 7;
            let doubled: u32;
            sequence {
                await_future(
                    |world: &mut World, input: &u32| {
                        Some(Slow {
                            polls: world.polls,
                            answer: world.answer.map(|_| input * 2),
                            dropped: world.dropped.clone(),
                        })
                    },
                    |_: &World| "waiting",
                )
                .with(input, out doubled);
                leaf_with(|world: &mut World, doubled: &u32| {
                    world.got = Some(*doubled);
                    NodeResult::Running("using")
                })
                .with(doubled);
            }
        };
        guard(|world: &World| world.go, wait)
    }

    #[test]
    fn an_answer_ready_at_once_passes_on_in_the_same_update() {
        let tree = tree();
        let mut state = BtState::new(&tree);
        let mut world = World {
            answer: Some(1),
            go: true,
            ..World::default()
        };
        let result = update(&tree, &mut state, &mut world, EntryMode::Evaluate);
        assert_eq!(result, NodeResult::Running("using"));
        assert_eq!(world.got, Some(14));
    }

    #[test]
    fn a_slow_answer_reports_waiting_then_passes_on() {
        let tree = tree();
        let mut state = BtState::new(&tree);
        let mut world = World {
            polls: 2,
            answer: Some(1),
            go: true,
            ..World::default()
        };
        for _ in 0..2 {
            let result = update(&tree, &mut state, &mut world, EntryMode::Resume);
            assert_eq!(result, NodeResult::Running("waiting"));
        }
        let result = update(&tree, &mut state, &mut world, EntryMode::Resume);
        assert_eq!(result, NodeResult::Running("using"));
        assert_eq!(world.got, Some(14));
    }

    #[test]
    fn no_answer_fails() {
        let tree = tree();
        let mut state = BtState::new(&tree);
        let mut world = World {
            go: true,
            ..World::default()
        };
        let result = update(&tree, &mut state, &mut world, EntryMode::Evaluate);
        assert_eq!(result, NodeResult::Failure);
        assert_eq!(world.got, None);
    }

    #[test]
    fn leaving_the_branch_drops_the_future() {
        let tree = tree();
        let mut state = BtState::new(&tree);
        let mut world = World {
            polls: 5,
            answer: Some(1),
            go: true,
            ..World::default()
        };
        let result = update(&tree, &mut state, &mut world, EntryMode::Evaluate);
        assert_eq!(result, NodeResult::Running("waiting"));
        assert_eq!(world.dropped.load(Ordering::Relaxed), 0);
        world.go = false;
        let result = update(&tree, &mut state, &mut world, EntryMode::Evaluate);
        assert_eq!(result, NodeResult::Failure);
        assert_eq!(
            world.dropped.load(Ordering::Relaxed),
            1,
            "the abandoned future was dropped"
        );
    }

    #[test]
    fn a_ready_future_from_core_works() {
        let tree = scope! {
            let input: u32 = |_: &mut ()| 3;
            let out: u32;
            sequence {
                await_future(|_: &mut (), input: &u32| Some(ready(Some(*input))), |_: &()| ())
                    .with(input, out out);
            }
        };
        let mut state = BtState::new(&tree);
        assert_eq!(
            update(&tree, &mut state, &mut (), EntryMode::Evaluate),
            NodeResult::Success
        );
    }
}
