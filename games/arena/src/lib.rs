//! A load test for the FlatBT Bevy integration, shaped like a game.
//!
//! [`ai`] is the part under test: three enemy minds over one
//! [`BehaviorContext`](flatbt::bevy::BehaviorContext). [`world`] is the game
//! around them -- components, the shared read-only view, and the one system
//! that answers a question a tree cannot ask for itself.
//!
//! ```sh
//! cargo run --release --bin arena   # windowed
//! cargo run --release --bin bench   # headless, serial vs parallel
//! ```

pub mod ai;
pub mod handrolled;
pub mod plain;
pub mod world;

/// What an enemy runs, and what it looks like.
///
/// The trees are plain functions, so a table of them is a table of function
/// items -- each with its own type, which is why spawning goes through a
/// `match` rather than a list.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mind {
    Chaser,
    Sniper,
    Coward,
}

impl Mind {
    pub const ALL: [Mind; 3] = [Mind::Chaser, Mind::Sniper, Mind::Coward];

    /// Cycles the three so a population is evenly mixed.
    pub fn nth(index: u32) -> Mind {
        Mind::ALL[index as usize % Mind::ALL.len()]
    }
}
