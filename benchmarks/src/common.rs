//! The vocabulary every library runs: one blackboard, one set of leaf
//! operations, and trees described once as a [`Spec`].

use crate::soldier::{self, Soldier, SoldierOp};
use crate::villager::{self, Villager, VillagerOp};

/// Per-agent world state. Every library reads and writes it through [`Op::run`].
#[derive(Clone, Debug, PartialEq)]
pub struct Bb {
    pub t: u32,
    pub mode: u8,
    pub enemy: bool,
    pub hp: i32,
    pub pos: i32,
    pub charge: u32,
    pub score: u64,
    /// Only the `soldier` scenario's world moves it.
    pub rich: bool,
    pub soldier: Soldier,
    /// Only the `villager` scenario's world moves it.
    pub village: bool,
    pub villager: Villager,
}

impl Bb {
    pub fn new(scenario: Scenario, t: u32) -> Self {
        Self {
            t,
            mode: 0,
            enemy: false,
            hp: 100,
            pos: 0,
            charge: 0,
            score: 0,
            rich: scenario == Scenario::Soldier,
            soldier: Soldier::new(),
            village: scenario == Scenario::Villager,
            villager: Villager::new(t.wrapping_mul(747_796_405).wrapping_add(1)),
        }
    }

    /// Advances the world outside the tree, once per tick.
    #[inline]
    pub fn step_world(&mut self) {
        self.t = self.t.wrapping_add(1);
        let h = self.t.wrapping_mul(2_654_435_761) >> 13;
        self.mode = (h % 8) as u8;
        self.enemy = h.is_multiple_of(5);
        if self.enemy {
            self.hp -= 9;
        }
        if self.rich {
            soldier::step(&mut self.soldier, h);
        }
        if self.village {
            villager::step(&mut self.villager, h);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Success,
    Failure,
    Running,
}

/// A leaf. Conditions succeed or fail; `MoveTo` and `Attack` run over several
/// ticks, keeping their progress on the blackboard so that every library can
/// express them the same way.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Op {
    IsMode(u8),
    Act(u32),
    MoveTo(i32),
    LowHp,
    EnemyNear,
    Heal,
    Attack,
    Soldier(SoldierOp),
    Villager(VillagerOp),
}

impl From<VillagerOp> for Op {
    fn from(op: VillagerOp) -> Self {
        Op::Villager(op)
    }
}

impl From<SoldierOp> for Op {
    fn from(op: SoldierOp) -> Self {
        Op::Soldier(op)
    }
}

impl Op {
    #[inline]
    pub fn run(self, bb: &mut Bb) -> Outcome {
        use Outcome::*;
        match self {
            Op::IsMode(m) => pass(bb.mode == m),
            Op::Act(n) => {
                bb.score += n as u64;
                Success
            }
            Op::MoveTo(x) if bb.pos == x => Success,
            Op::MoveTo(x) => {
                bb.pos += (x - bb.pos).signum();
                Running
            }
            Op::LowHp => pass(bb.hp < 30),
            Op::EnemyNear => pass(bb.enemy),
            Op::Heal => {
                bb.hp = 100;
                bb.score += 1000;
                Success
            }
            Op::Attack => {
                bb.charge += 1;
                if bb.charge < 3 {
                    return Running;
                }
                bb.charge = 0;
                bb.score += 100;
                Success
            }
            Op::Soldier(op) => op.run(bb),
            Op::Villager(op) => op.run(bb),
        }
    }
}

#[inline]
fn pass(ok: bool) -> Outcome {
    if ok {
        Outcome::Success
    } else {
        Outcome::Failure
    }
}

/// A tree for the libraries built at runtime. FlatBT's trees are typed, so
/// `libs::flatbt` writes the same shapes out by hand.
pub enum Spec {
    Seq(Vec<Spec>),
    Sel(Vec<Spec>),
    Leaf(Op),
    /// `select` (true) or `seq` over children in the order `Order` picks.
    Ordered(bool, Order, Vec<Spec>),
    Repeat(usize, Box<Spec>),
    Retry(usize, Box<Spec>),
    IfElse(fn(&Bb) -> bool, Box<Spec>, Box<Spec>),
    Invert(Box<Spec>),
    ForceSuccess(Box<Spec>),
    RepeatWhile(fn(&Bb) -> bool, Box<Spec>),
}

/// How an ordered control picks its next child. Ports FlatBT's `by_score`,
/// `shuffled` and `weighted` exactly, for the libraries without them.
#[derive(Clone, Copy)]
pub enum Order {
    Score(fn(&Bb, usize) -> i32),
    Shuffle,
    Weighted(fn(&Bb, usize) -> f32),
}

impl Order {
    /// The next of `count` children not in `used`, as FlatBT picks it when no
    /// child is running: always the case under `Resume`.
    pub fn next(self, bb: &mut Bb, used: u64, count: usize) -> Option<usize> {
        let free = |index: &usize| used & (1 << index) == 0;
        match self {
            Order::Score(score) => {
                let mut best: Option<(usize, i32)> = None;
                for index in (0..count).filter(free) {
                    let score = score(bb, index);
                    // Ties go to the lower index.
                    if best.is_none_or(|(_, best)| score > best) {
                        best = Some((index, score));
                    }
                }
                best.map(|(index, _)| index)
            }
            Order::Shuffle => {
                let left = count - used.count_ones() as usize;
                if left == 0 {
                    return None;
                }
                let nth = villager::draw(bb) as usize % left;
                (0..count).filter(free).nth(nth)
            }
            Order::Weighted(weight) => {
                let weight = |bb: &Bb, index: usize| {
                    let weight = weight(bb, index);
                    if weight > 0.0 { weight } else { 0.0 }
                };
                let total: f32 = (0..count).filter(free).map(|index| weight(bb, index)).sum();
                if total <= 0.0 || !total.is_finite() {
                    return None;
                }
                let draw = villager::draw(bb) as f32 / (u32::MAX as f32 + 1.0) * total;
                let mut last = None;
                let mut sum = 0.0;
                for index in (0..count).filter(free) {
                    let weight = weight(bb, index);
                    if weight == 0.0 {
                        continue;
                    }
                    sum += weight;
                    last = Some(index);
                    if draw < sum {
                        break;
                    }
                }
                last
            }
        }
    }
}

impl Spec {
    /// (nodes, leaves, depth)
    pub fn shape(&self) -> (usize, usize, usize) {
        match self {
            Spec::Leaf(_) => (1, 1, 1),
            Spec::Repeat(_, child)
            | Spec::Retry(_, child)
            | Spec::Invert(child)
            | Spec::ForceSuccess(child)
            | Spec::RepeatWhile(_, child) => {
                let (n, l, d) = child.shape();
                (n + 1, l, d + 1)
            }
            Spec::IfElse(_, then, otherwise) => {
                let (tn, tl, td) = then.shape();
                let (on, ol, od) = otherwise.shape();
                (1 + tn + on, tl + ol, 1 + td.max(od))
            }
            Spec::Seq(children) | Spec::Sel(children) | Spec::Ordered(_, _, children) => children
                .iter()
                .map(Spec::shape)
                .fold((1, 0, 0), |(n, l, d), (cn, cl, cd)| {
                    (n + cn, l + cl, d.max(cd + 1))
                }),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scenario {
    /// Selector over 8 condition/action branches; completes every tick.
    Select8,
    /// Sequence of long-running moves; Running on most ticks.
    Patrol,
    /// Flee / attack / patrol priorities, three levels deep.
    Guard,
    /// A game NPC: 7 priorities, 7 levels, see `soldier`.
    Soldier,
    /// Utility, weighted and shuffled orders with decorators, see `villager`.
    Villager,
}

pub const SCENARIOS: [Scenario; 5] = [
    Scenario::Select8,
    Scenario::Patrol,
    Scenario::Guard,
    Scenario::Soldier,
    Scenario::Villager,
];

/// The scenarios built from sequences, selectors and leaves only: all that
/// every library can express.
pub const BASIC: [Scenario; 4] = [
    Scenario::Select8,
    Scenario::Patrol,
    Scenario::Guard,
    Scenario::Soldier,
];

impl Scenario {
    pub fn name(self) -> &'static str {
        match self {
            Scenario::Select8 => "select8",
            Scenario::Patrol => "patrol",
            Scenario::Guard => "guard",
            Scenario::Soldier => "soldier",
            Scenario::Villager => "villager",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        SCENARIOS.into_iter().find(|s| s.name() == name)
    }

    pub fn spec(self) -> Spec {
        use Spec::{Leaf, Sel, Seq};
        match self {
            Scenario::Select8 => Sel((0..8)
                .map(|i| Seq(vec![Leaf(Op::IsMode(i)), Leaf(Op::Act(i as u32))]))
                .collect()),
            Scenario::Patrol => Seq(patrol()),
            Scenario::Guard => Sel(vec![
                Seq(vec![Leaf(Op::LowHp), Leaf(Op::MoveTo(0)), Leaf(Op::Heal)]),
                Seq(vec![Leaf(Op::EnemyNear), Leaf(Op::Attack)]),
                Seq(patrol()),
            ]),
            Scenario::Soldier => soldier::spec(),
            Scenario::Villager => villager::spec(),
        }
    }
}

fn patrol() -> Vec<Spec> {
    use Spec::Leaf;
    vec![
        Leaf(Op::MoveTo(8)),
        Leaf(Op::Act(1)),
        Leaf(Op::MoveTo(-8)),
        Leaf(Op::Act(2)),
    ]
}

/// One line that identifies a run's outcome. `csharp/Program.cs` prints the
/// same format, so the two can be compared as text.
pub fn checksum(scenario: Scenario, (bb, counts): &(Bb, [u64; 3])) -> String {
    let s = &bb.soldier;
    let join = |v: &[u32]| v.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
    let work: Vec<u32> = s.work.iter().map(|&w| w as u32).collect();
    format!(
        "{} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {}",
        scenario.name(),
        counts[0],
        counts[1],
        counts[2],
        bb.t,
        bb.mode,
        bb.enemy as u8,
        bb.hp,
        bb.pos,
        bb.charge,
        bb.score,
        s.health,
        s.medkits,
        s.mag,
        s.reserve,
        s.grenades,
        s.food,
        s.hunger,
        s.fatigue,
        s.foe_left,
        s.foe_hp,
        s.foe_dist,
        s.foe_grouped as u8,
        s.noise.map_or("-".to_owned(), |n| n.to_string()),
        join(&work),
        s.kills,
        s.deaths,
        join(&s.effects),
    )
}
