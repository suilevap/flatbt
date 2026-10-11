//! Runners and their promises.

use std::future::Future;
use std::pin::Pin;
use std::sync::mpsc;
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

use bevy_tasks::{AsyncComputeTaskPool, TaskPool};
use flatbt_bevy::prelude::*;

/// Polls once without blocking.
fn poll<T: Unpin>(promise: &mut Promise<T>) -> Option<T> {
    match Pin::new(promise).poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(answer) => Some(answer),
        Poll::Pending => None,
    }
}

#[test]
fn inline_answers_at_once() {
    assert_eq!(poll(&mut Runner::Inline.run(|| 7)), Some(7));
    assert_eq!(poll(&mut Promise::ready(7)), Some(7));
}

#[test]
fn deferred_answers_after_its_polls() {
    let mut promise = Runner::Deferred(2).run(|| 7);
    assert_eq!(poll(&mut promise), None);
    assert_eq!(poll(&mut promise), None);
    assert_eq!(poll(&mut promise), Some(7));
}

/// Work in the background that takes its time leaves the caller free to go on
/// polling, frame after frame, and answers once it is done.
#[test]
fn background_never_blocks_the_caller() {
    AsyncComputeTaskPool::get_or_init(TaskPool::default);
    let (release, gate) = mpsc::channel::<u32>();
    let mut promise = Runner::Background.run(move || gate.recv().unwrap_or(0));
    for _ in 0..20 {
        let polled = Instant::now();
        assert_eq!(poll(&mut promise), None);
        assert!(polled.elapsed() < Duration::from_millis(50));
        std::thread::sleep(Duration::from_millis(1));
    }
    release.send(7).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(answer) = poll(&mut promise) {
            assert_eq!(answer, 7);
            break;
        }
        assert!(Instant::now() < deadline, "the work never finished");
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// A tree waits on the promise with `await_future`, whichever runner made it,
/// and gets the same answer.
#[test]
fn a_tree_waits_the_same_on_every_runner() {
    AsyncComputeTaskPool::get_or_init(TaskPool::default);
    struct Mind {
        runner: Runner,
    }
    let tree = scope! {
        let input: u32 = |_: &mut Mind| 21;
        let doubled: u32;
        sequence {
            await_future(
                |mind: &mut Mind, input: &u32| {
                    let input = *input;
                    Some(mind.runner.run(move || Some(input * 2)))
                },
                |_: &Mind| 0,
            )
            .with(input, out doubled);
            leaf_with(|_: &mut Mind, doubled: &u32| NodeResult::Running(*doubled))
                .with(doubled);
        }
    };
    for runner in [Runner::Inline, Runner::Deferred(3), Runner::Background] {
        let mut state = BtState::new(&tree);
        let mut mind = Mind { runner };
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut waited = 0;
        loop {
            match update(&tree, &mut state, &mut mind, EntryMode::Resume) {
                NodeResult::Running(0) => waited += 1,
                result => {
                    assert_eq!(result, NodeResult::Running(42), "{runner:?}");
                    break;
                }
            }
            assert!(Instant::now() < deadline, "{runner:?} never answered");
            std::thread::sleep(Duration::from_millis(1));
        }
        match runner {
            Runner::Inline => assert_eq!(waited, 0),
            Runner::Deferred(polls) => assert_eq!(waited, polls),
            Runner::Background => {}
        }
    }
}
