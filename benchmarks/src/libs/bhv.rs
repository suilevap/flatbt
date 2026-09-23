use crate::common::{Bb, Op, Outcome, SCENARIOS, Spec};
use crate::harness::{Measure, entry};
use bhv::{Bhv, Sel, Seq, Status};

pub const NAME: &str = "bhv";

struct OpNode(Op);

impl Bhv for OpNode {
    type Context = Bb;

    fn update(&mut self, bb: &mut Bb) -> Status {
        match self.0.run(bb) {
            Outcome::Success => Status::Success,
            Outcome::Failure => Status::Failure,
            Outcome::Running => Status::Running,
        }
    }
}

type Tree = Box<dyn Bhv<Context = Bb>>;

fn build(spec: &Spec) -> Tree {
    match spec {
        Spec::Seq(children) => Box::new(Seq::with_nodes(children.iter().map(build).collect())),
        Spec::Sel(children) => Box::new(Sel::with_nodes(children.iter().map(build).collect())),
        Spec::Leaf(op) => Box::new(OpNode(*op)),
    }
}

pub fn entries() -> Vec<Box<dyn Measure>> {
    SCENARIOS
        .into_iter()
        .map(|scenario| {
            // Nodes hold their own state: one boxed tree per agent.
            entry(
                NAME,
                false,
                scenario,
                move || scenario.spec(),
                |spec: &Spec| build(spec),
                |_, root: &mut Tree, bb| match root.update(bb) {
                    Status::Success => Outcome::Success,
                    Status::Failure => Outcome::Failure,
                    Status::Running => Outcome::Running,
                },
            )
        })
        .collect()
}
