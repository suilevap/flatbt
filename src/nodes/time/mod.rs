//! Nodes that measure time through the context's [`BtClock`].
//!
//! Time is whatever the game says it is: seconds, turns, simulation ticks. The
//! context implements `BtClock`, and spans are given in its `Duration`.
//!
//! ```
//! use flatbt::prelude::*;
//!
//! struct Game { turn: u32 }
//! impl BtClock for Game {
//!     type Instant = u32;
//!     type Duration = u32;
//!     fn now(&self) -> u32 { self.turn }
//! }
//!
//! // Dash at most once every 3 turns.
//! let tree = cooldown(3, leaf(|_: &mut Game| NodeResult::Success));
//! let mut state: BtState<_, _> = BtState::new(&tree);
//! let mut game = Game { turn: 0 };
//! assert_eq!(update(&tree, &mut state, &mut game, EntryMode::Evaluate), NodeResult::Success);
//! game.turn = 2;
//! assert_eq!(update(&tree, &mut state, &mut game, EntryMode::Evaluate), NodeResult::Failure);
//! game.turn = 3;
//! assert_eq!(update(&tree, &mut state, &mut game, EntryMode::Evaluate), NodeResult::Success);
//! ```

mod cooldown;
mod every;
mod timeout;
mod wait;

pub use cooldown::{Cooldown, CooldownMemory, CooldownState, cooldown, success_cooldown};
pub use every::{ReevaluateEvery, ReevaluateEveryState, reevaluate_every};
pub use timeout::{Timeout, TimeoutState, timeout};
pub use wait::{ActionWait, action_wait};

use core::fmt::Debug;
use core::ops::Add;

/// A context that knows the time, for the nodes in this module.
///
/// `Instant + Duration` is the instant a span after it. For seconds as `f32`
/// both are `f32`; for `std::time` they are `Instant` and `Duration`; for
/// time since startup, as Bevy's `Time::elapsed`, both are `Duration`.
pub trait BtClock {
    /// A point in time. Ordered; a clock that goes backwards delays expiry.
    type Instant: Copy
        + PartialOrd
        + Add<Self::Duration, Output = Self::Instant>
        + Debug
        + Send
        + 'static;
    /// A span of time, as nodes are configured with.
    type Duration: Copy + Debug + Send + 'static;

    /// The current time. Asked by each node that needs it, so it should not
    /// change within one update.
    fn now(&self) -> Self::Instant;
}

/// Whether `span` has passed since `since`.
#[inline]
fn elapsed<Context: BtClock>(
    ctx: &Context,
    since: Context::Instant,
    span: Context::Duration,
) -> bool {
    ctx.now() >= since + span
}
