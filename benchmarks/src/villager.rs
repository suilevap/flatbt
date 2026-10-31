//! A villager whose needs compete: the node catalog's orders and decorators.
//!
//! Needs are picked by utility; a friend to visit by weighted chance; chores
//! in a shuffled order. A threat takes over through `if_else`: fight when
//! healthy, flee when not. Randomness comes from one generator on the
//! blackboard, so every library draws the same numbers in the same order.

use crate::common::{Bb, Order, Outcome, Spec};

#[derive(Clone, Debug, PartialEq)]
pub struct Villager {
    pub hunger: i32,
    pub fatigue: i32,
    pub boredom: i32,
    pub duty: i32,
    pub health: i32,
    pub threat_left: u32,
    pub threat_hp: i32,
    pub cornered: bool,
    pub loot: bool,
    pub food: u8,
    pub wood: u32,
    pub water: u32,
    pub swept: u32,
    pub friendship: [u32; 3],
    pub rng: u32,
    pub draws: u32,
    /// Progress of multi-tick work, one counter per slot.
    pub work: [u8; 6],
    /// How often each `Effect` was applied.
    pub effects: [u32; Effect::COUNT],
}

impl Villager {
    pub fn new(seed: u32) -> Self {
        Self {
            hunger: 0,
            fatigue: 0,
            boredom: 0,
            duty: 0,
            health: 100,
            threat_left: 0,
            threat_hp: 0,
            cornered: false,
            loot: false,
            food: 0,
            wood: 0,
            water: 0,
            swept: 0,
            friendship: [0; 3],
            // xorshift32 must not start at zero.
            rng: seed | 1,
            draws: 0,
            work: [0; 6],
            effects: [0; Effect::COUNT],
        }
    }

    fn threat(&self) -> bool {
        self.threat_left > 0
    }
}

/// The generator every random order and leaf draws from.
#[inline]
pub fn draw(bb: &mut Bb) -> u32 {
    let v = &mut bb.villager;
    v.draws += 1;
    let mut x = v.rng;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    v.rng = x;
    x
}

/// The villager's part of the world step; `h` is the tick's hash.
pub fn step(v: &mut Villager, h: u32) {
    v.hunger += h.is_multiple_of(3) as i32;
    v.fatigue += h.is_multiple_of(4) as i32;
    v.boredom += h.is_multiple_of(3) as i32;
    v.duty += h.is_multiple_of(5) as i32;
    if v.threat() {
        v.threat_left -= 1;
        if h.is_multiple_of(3) {
            v.health -= 4;
        }
        v.cornered = h.is_multiple_of(6);
    } else {
        if h.is_multiple_of(131) {
            v.threat_left = 20 + h % 20;
            v.threat_hp = 3 + (h % 6) as i32;
        }
        if v.health < 100 && h.is_multiple_of(5) {
            v.health += 1;
        }
    }
}

/// Utility of each need, in `needs` order: eat, sleep, socialize, work.
#[inline]
pub fn need(bb: &Bb, index: usize) -> i32 {
    let v = &bb.villager;
    match index {
        0 => v.hunger,
        1 => v.fatigue,
        2 => v.boredom,
        _ => v.duty,
    }
}

/// Chance of visiting each friend grows with friendship.
#[inline]
pub fn friend_weight(bb: &Bb, index: usize) -> f32 {
    1.0 + bb.villager.friendship[index].min(20) as f32
}

pub fn healthy(bb: &Bb) -> bool {
    bb.villager.health > 70
}

pub fn tired(bb: &Bb) -> bool {
    bb.villager.fatigue > 0
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cond {
    Threat,
    Cornered,
    Loot,
    FriendHome(u8),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Place {
    Safe,
    Kitchen,
    Bed,
    Friend(u8),
    Woodpile,
    Well,
    Hall,
}

/// When unfinished work gives up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Abort {
    Never,
    OnThreat,
    OnNoThreat,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Effect {
    Swing,
    TakeLoot,
    Hide,
    Chew,
    Eat,
    Rest,
    Chat(u8),
    Chop,
    Carry,
    Sweep,
}

impl Effect {
    pub const COUNT: usize = 12;
    pub const NAMES: [&str; Self::COUNT] = [
        "swing",
        "take loot",
        "hide",
        "chew",
        "eat",
        "rest",
        "chat 0",
        "chat 1",
        "chat 2",
        "chop",
        "carry",
        "sweep",
    ];

    fn index(self) -> usize {
        match self {
            Effect::Swing => 0,
            Effect::TakeLoot => 1,
            Effect::Hide => 2,
            Effect::Chew => 3,
            Effect::Eat => 4,
            Effect::Rest => 5,
            Effect::Chat(i) => 6 + i as usize,
            Effect::Chop => 9,
            Effect::Carry => 10,
            Effect::Sweep => 11,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VillagerOp {
    If(Cond),
    /// Walks one step per tick; with `yields`, fails once a threat appears.
    Goto(Place, bool),
    /// Runs for `ticks` ticks in `slot`, then applies the effect.
    Work(u8, u8, Abort, Effect),
    Do(Effect),
    /// Finds food two times in three.
    Forage,
}

impl VillagerOp {
    #[inline]
    pub fn run(self, bb: &mut Bb) -> Outcome {
        use Outcome::*;
        match self {
            VillagerOp::If(c) => {
                let v = &bb.villager;
                let ok = match c {
                    Cond::Threat => v.threat(),
                    Cond::Cornered => v.cornered,
                    Cond::Loot => v.loot,
                    Cond::FriendHome(i) => !(bb.t / 40 + i as u32).is_multiple_of(3),
                };
                if ok { Success } else { Failure }
            }
            VillagerOp::Goto(place, yields) => {
                if yields && bb.villager.threat() {
                    return Failure;
                }
                let target = match place {
                    Place::Safe => -30,
                    Place::Kitchen => 10,
                    Place::Bed => -10,
                    Place::Friend(i) => 20 + 5 * i as i32,
                    Place::Woodpile => 30,
                    Place::Well => -20,
                    Place::Hall => 0,
                };
                if bb.pos == target {
                    return Success;
                }
                bb.pos += (target - bb.pos).signum();
                Running
            }
            VillagerOp::Work(slot, ticks, abort, effect) => {
                let v = &mut bb.villager;
                let slot = slot as usize;
                let stop = match abort {
                    Abort::Never => false,
                    Abort::OnThreat => v.threat(),
                    Abort::OnNoThreat => !v.threat(),
                };
                if stop {
                    v.work[slot] = 0;
                    return Failure;
                }
                v.work[slot] += 1;
                if v.work[slot] < ticks {
                    return Running;
                }
                v.work[slot] = 0;
                apply(effect, bb);
                Success
            }
            VillagerOp::Do(effect) => {
                apply(effect, bb);
                Success
            }
            VillagerOp::Forage => {
                if draw(bb).is_multiple_of(3) {
                    Failure
                } else {
                    bb.villager.food += 1;
                    Success
                }
            }
        }
    }
}

fn apply(effect: Effect, bb: &mut Bb) {
    let v = &mut bb.villager;
    v.effects[effect.index()] += 1;
    match effect {
        Effect::Swing => {
            v.threat_hp -= 1;
            if v.threat_hp <= 0 {
                v.threat_left = 0;
                v.loot = true;
                bb.score += 50;
            }
        }
        Effect::TakeLoot => {
            v.loot = false;
            v.food += 1;
        }
        Effect::Hide => v.health = (v.health + 25).min(100),
        Effect::Chew => v.hunger = (v.hunger - 40).max(0),
        Effect::Eat => {
            v.food -= 1;
            v.hunger = (v.hunger - 50).max(0);
        }
        Effect::Rest => v.fatigue = (v.fatigue - 40).max(0),
        Effect::Chat(i) => {
            v.boredom = 0;
            v.friendship[i as usize] += 1;
        }
        Effect::Chop => {
            v.wood += 1;
            v.duty = (v.duty - 30).max(0);
        }
        Effect::Carry => {
            v.water += 1;
            v.duty = (v.duty - 30).max(0);
        }
        Effect::Sweep => {
            v.swept += 1;
            v.duty = (v.duty - 30).max(0);
        }
    }
}

/// The tree for the libraries built at runtime; `libs::flatbt` writes the
/// same tree with the node catalog.
pub fn spec() -> Spec {
    use Abort::*;
    use Effect::*;
    use Spec::{ForceSuccess, IfElse, Invert, Ordered, Repeat, RepeatWhile, Retry, Sel, Seq};
    let leaf = |op: VillagerOp| Spec::Leaf(op.into());
    let c = |cond| leaf(VillagerOp::If(cond));
    let go = |place, yields| leaf(VillagerOp::Goto(place, yields));
    let work = |slot, ticks, abort, effect| leaf(VillagerOp::Work(slot, ticks, abort, effect));
    let act = |effect| leaf(VillagerOp::Do(effect));
    let boxed = Box::new;
    let chat = |i: u8| {
        Seq(vec![
            c(Cond::FriendHome(i)),
            go(Place::Friend(i), true),
            work(4, 3, OnThreat, Chat(i)),
        ])
    };
    let chore = |place, effect| Seq(vec![go(place, true), work(5, 2, OnThreat, effect)]);
    Sel(vec![
        Seq(vec![
            c(Cond::Threat),
            IfElse(
                healthy,
                boxed(Seq(vec![
                    Repeat(3, boxed(work(0, 2, OnNoThreat, Swing))),
                    ForceSuccess(boxed(Seq(vec![c(Cond::Loot), act(TakeLoot)]))),
                ])),
                boxed(Seq(vec![
                    Invert(boxed(c(Cond::Cornered))),
                    go(Place::Safe, false),
                    work(1, 4, Never, Hide),
                ])),
            ),
        ]),
        Ordered(
            true,
            Order::Score(need),
            vec![
                Seq(vec![
                    Retry(3, boxed(leaf(VillagerOp::Forage))),
                    go(Place::Kitchen, true),
                    Repeat(2, boxed(work(2, 3, OnThreat, Chew))),
                    act(Eat),
                ]),
                Seq(vec![
                    go(Place::Bed, true),
                    RepeatWhile(tired, boxed(work(3, 2, OnThreat, Rest))),
                ]),
                Ordered(
                    true,
                    Order::Weighted(friend_weight),
                    vec![chat(0), chat(1), chat(2)],
                ),
                Ordered(
                    false,
                    Order::Shuffle,
                    vec![
                        chore(Place::Woodpile, Chop),
                        chore(Place::Well, Carry),
                        chore(Place::Hall, Sweep),
                    ],
                ),
            ],
        ),
    ])
}
