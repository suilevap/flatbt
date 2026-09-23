//! The vocabulary every library runs: one blackboard, one set of leaf
//! operations, and trees described once as a [`Spec`].

/// Per-agent world state. Every library reads and writes it through [`Op::run`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bb {
    pub t: u32,
    pub mode: u8,
    pub enemy: bool,
    pub hp: i32,
    pub pos: i32,
    pub charge: u32,
    pub score: u64,
}

impl Bb {
    pub fn new(t: u32) -> Self {
        Self {
            t,
            mode: 0,
            enemy: false,
            hp: 100,
            pos: 0,
            charge: 0,
            score: 0,
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scenario {
    /// Selector over 8 condition/action branches; completes every tick.
    Select8,
    /// Sequence of long-running moves; Running on most ticks.
    Patrol,
    /// Flee / attack / patrol priorities, three levels deep.
    Guard,
}

pub const SCENARIOS: [Scenario; 3] = [Scenario::Select8, Scenario::Patrol, Scenario::Guard];

impl Scenario {
    pub fn name(self) -> &'static str {
        match self {
            Scenario::Select8 => "select8",
            Scenario::Patrol => "patrol",
            Scenario::Guard => "guard",
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
