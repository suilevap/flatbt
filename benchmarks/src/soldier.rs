//! A game NPC: respawn, survival, combat, investigation, needs and patrol.
//!
//! Controls keep their place, so long actions give way by failing: a patrol
//! step fails when an enemy shows up, aiming fails when the enemy is gone. The
//! failure unwinds to the root, and the next tick chooses again from the top.

use crate::common::{Bb, Outcome, Spec};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Soldier {
    pub health: i32,
    pub medkits: u8,
    pub mag: u8,
    pub reserve: u16,
    pub grenades: u8,
    pub food: u8,
    pub hunger: u32,
    pub fatigue: u32,
    pub foe_left: u32,
    pub foe_hp: i32,
    pub foe_dist: i32,
    pub foe_grouped: bool,
    pub noise: Option<i32>,
    /// Progress of multi-tick work, one counter per slot.
    pub work: [u8; 9],
    pub kills: u32,
    pub deaths: u32,
    /// How often each `Effect` was applied, by `Effect::index`.
    pub effects: [u32; Effect::COUNT],
}

impl Soldier {
    pub fn new() -> Self {
        Self {
            health: 100,
            medkits: 2,
            mag: 8,
            reserve: 24,
            grenades: 2,
            food: 1,
            hunger: 0,
            fatigue: 0,
            foe_left: 0,
            foe_hp: 0,
            foe_dist: 0,
            foe_grouped: false,
            noise: None,
            work: [0; 9],
            kills: 0,
            deaths: 0,
            effects: [0; Effect::COUNT],
        }
    }

    fn foe(&self) -> bool {
        self.foe_left > 0
    }

    fn hurt_foe(&mut self, damage: i32, score: &mut u64) {
        self.foe_hp -= damage;
        if self.foe_hp <= 0 {
            self.foe_left = 0;
            self.kills += 1;
            *score += 100;
        }
    }
}

/// The soldier's part of the world step; `h` is the tick's hash.
pub fn step(s: &mut Soldier, h: u32) {
    s.hunger += 1;
    s.fatigue += 1;
    if s.foe() {
        s.foe_left -= 1;
        if h.is_multiple_of(3) {
            s.health -= 4;
        }
        if h.is_multiple_of(7) && s.foe_dist > 0 {
            s.foe_dist -= 1;
        }
    } else if h.is_multiple_of(89) {
        s.foe_left = 60 + h % 60;
        s.foe_hp = 3 + (h % 4) as i32;
        s.foe_dist = 15 + (h % 15) as i32;
        s.foe_grouped = h.is_multiple_of(2);
    }
    if h.is_multiple_of(61) {
        s.noise = Some((h % 31) as i32 - 15);
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cond {
    Dead,
    LowHealth,
    HasMedkit,
    FoeVisible,
    MagEmpty,
    HasReserve,
    FoeClose,
    FoeInRange,
    HasGrenade,
    FoeGrouped,
    HeardNoise,
    Hungry,
    HasFood,
    Tired,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Place {
    Cover,
    Base,
    Ammo,
    Kitchen,
    Bed,
    Noise,
    At(i32),
}

/// When unfinished work gives up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Abort {
    Never,
    OnFoe,
    OnFoeGone,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Effect {
    Idle,
    Respawn,
    UseMedkit,
    Heal(i32),
    Reload,
    Melee,
    TakeAmmo,
    Throw,
    Fire,
    ClearNoise,
    Eat,
    TakeFood,
    Sleep,
    Score(u64),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SoldierOp {
    If(Cond),
    /// Walks one step per tick; with `yields`, fails once an enemy is visible.
    Goto(Place, bool),
    /// Runs for `ticks` ticks in `slot`, then applies the effect.
    Work(u8, u8, Abort, Effect),
    Do(Effect),
    /// Closes in on the enemy until within range; fails if it is gone.
    Close(i32),
}

impl SoldierOp {
    #[inline]
    pub fn run(self, bb: &mut Bb) -> Outcome {
        use Outcome::*;
        let s = &mut bb.soldier;
        match self {
            SoldierOp::If(c) => {
                let ok = match c {
                    Cond::Dead => s.health <= 0,
                    Cond::LowHealth => s.health < 35,
                    Cond::HasMedkit => s.medkits > 0,
                    Cond::FoeVisible => s.foe(),
                    Cond::MagEmpty => s.mag == 0,
                    Cond::HasReserve => s.reserve > 0,
                    Cond::FoeClose => s.foe_dist <= 2,
                    Cond::FoeInRange => s.foe_dist <= 12,
                    Cond::HasGrenade => s.grenades > 0,
                    Cond::FoeGrouped => s.foe_grouped,
                    Cond::HeardNoise => s.noise.is_some(),
                    Cond::Hungry => s.hunger > 400,
                    Cond::HasFood => s.food > 0,
                    Cond::Tired => s.fatigue > 700,
                };
                if ok { Success } else { Failure }
            }
            SoldierOp::Goto(place, yields) => {
                if yields && s.foe() {
                    return Failure;
                }
                let target = match place {
                    Place::Cover => 5,
                    Place::Base => -20,
                    Place::Ammo => -15,
                    Place::Kitchen => 20,
                    Place::Bed => -25,
                    Place::Noise => s.noise.unwrap_or(bb.pos),
                    Place::At(x) => x,
                };
                if bb.pos == target {
                    return Success;
                }
                bb.pos += (target - bb.pos).signum();
                Running
            }
            SoldierOp::Work(slot, ticks, abort, effect) => {
                let slot = slot as usize;
                let abort = match abort {
                    Abort::Never => false,
                    Abort::OnFoe => s.foe(),
                    Abort::OnFoeGone => !s.foe(),
                };
                if abort {
                    s.work[slot] = 0;
                    return Failure;
                }
                s.work[slot] += 1;
                if s.work[slot] < ticks {
                    return Running;
                }
                s.work[slot] = 0;
                apply(effect, bb);
                Success
            }
            SoldierOp::Do(effect) => {
                apply(effect, bb);
                Success
            }
            SoldierOp::Close(range) => {
                if !s.foe() {
                    Failure
                } else if s.foe_dist <= range {
                    Success
                } else {
                    s.foe_dist -= 1;
                    Running
                }
            }
        }
    }
}

impl Effect {
    pub const COUNT: usize = 14;
    pub const NAMES: [&str; Self::COUNT] = [
        "idle",
        "respawn",
        "medkit",
        "heal",
        "reload",
        "melee",
        "take ammo",
        "grenade",
        "fire",
        "clear noise",
        "eat",
        "take food",
        "sleep",
        "patrol lap",
    ];

    fn index(self) -> usize {
        match self {
            Effect::Idle => 0,
            Effect::Respawn => 1,
            Effect::UseMedkit => 2,
            Effect::Heal(_) => 3,
            Effect::Reload => 4,
            Effect::Melee => 5,
            Effect::TakeAmmo => 6,
            Effect::Throw => 7,
            Effect::Fire => 8,
            Effect::ClearNoise => 9,
            Effect::Eat => 10,
            Effect::TakeFood => 11,
            Effect::Sleep => 12,
            Effect::Score(_) => 13,
        }
    }
}

fn apply(effect: Effect, bb: &mut Bb) {
    let s = &mut bb.soldier;
    s.effects[effect.index()] += 1;
    match effect {
        Effect::Idle => {}
        Effect::Respawn => {
            *s = Soldier {
                deaths: s.deaths + 1,
                kills: s.kills,
                effects: s.effects,
                hunger: s.hunger,
                fatigue: s.fatigue,
                foe_left: s.foe_left,
                foe_hp: s.foe_hp,
                foe_dist: s.foe_dist + 10,
                foe_grouped: s.foe_grouped,
                ..Soldier::new()
            };
            bb.pos = -20;
        }
        Effect::UseMedkit => {
            s.medkits -= 1;
            s.health = (s.health + 50).min(100);
        }
        Effect::Heal(n) => s.health = (s.health + n).min(100),
        Effect::Reload => {
            let take = (8 - s.mag as u16).min(s.reserve);
            s.reserve -= take;
            s.mag += take as u8;
        }
        Effect::Melee => s.hurt_foe(2, &mut bb.score),
        Effect::TakeAmmo => s.reserve += 16,
        Effect::Throw => {
            s.grenades -= 1;
            s.hurt_foe(10, &mut bb.score);
            bb.score += 200;
        }
        Effect::Fire => {
            s.mag -= 1;
            s.hurt_foe(1, &mut bb.score);
        }
        Effect::ClearNoise => s.noise = None,
        Effect::Eat => {
            s.food -= 1;
            s.hunger = 0;
        }
        Effect::TakeFood => s.food += 2,
        Effect::Sleep => s.fatigue = 0,
        Effect::Score(n) => bb.score += n,
    }
}

pub fn spec() -> Spec {
    use Abort::*;
    use Cond::*;
    use Effect::*;
    use Spec::{Sel, Seq};
    let c = |c| Spec::Leaf(SoldierOp::If(c).into());
    let go = |p, yields| Spec::Leaf(SoldierOp::Goto(p, yields).into());
    let work =
        |slot, ticks, abort, effect| Spec::Leaf(SoldierOp::Work(slot, ticks, abort, effect).into());
    let act = |e| Spec::Leaf(SoldierOp::Do(e).into());
    Sel(vec![
        Seq(vec![c(Dead), work(0, 20, Never, Respawn)]),
        Seq(vec![
            c(LowHealth),
            Sel(vec![
                Seq(vec![c(HasMedkit), work(1, 3, Never, UseMedkit)]),
                Seq(vec![
                    c(FoeVisible),
                    go(Place::Cover, false),
                    work(2, 6, Never, Heal(25)),
                ]),
                Seq(vec![go(Place::Base, false), work(2, 6, Never, Heal(25))]),
            ]),
        ]),
        Seq(vec![
            c(FoeVisible),
            Sel(vec![
                Seq(vec![
                    c(MagEmpty),
                    Sel(vec![
                        Seq(vec![c(HasReserve), work(3, 4, Never, Reload)]),
                        Seq(vec![c(FoeClose), work(4, 3, OnFoeGone, Melee)]),
                        Seq(vec![go(Place::Ammo, false), act(TakeAmmo)]),
                    ]),
                ]),
                Seq(vec![
                    c(FoeInRange),
                    Sel(vec![
                        Seq(vec![c(HasGrenade), c(FoeGrouped), act(Throw)]),
                        Seq(vec![work(5, 2, OnFoeGone, Idle), act(Fire)]),
                    ]),
                ]),
                Spec::Leaf(SoldierOp::Close(12).into()),
            ]),
        ]),
        Seq(vec![
            c(HeardNoise),
            go(Place::Noise, true),
            work(6, 3, OnFoe, Idle),
            act(ClearNoise),
        ]),
        Seq(vec![
            c(Hungry),
            Sel(vec![
                Seq(vec![c(HasFood), work(7, 5, OnFoe, Eat)]),
                Seq(vec![go(Place::Kitchen, true), act(TakeFood)]),
            ]),
        ]),
        Seq(vec![
            c(Tired),
            go(Place::Bed, true),
            work(8, 12, OnFoe, Sleep),
        ]),
        Seq(vec![
            go(Place::At(12), true),
            work(6, 3, OnFoe, Idle),
            go(Place::At(-12), true),
            work(6, 3, OnFoe, Idle),
            go(Place::At(0), true),
            act(Score(1)),
        ]),
    ])
}
