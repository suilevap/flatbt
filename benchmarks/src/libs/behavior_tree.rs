use crate::common::{BASIC, Bb, Op, Outcome, Spec};
use crate::harness::{Measure, entry};
use behavior_tree::{Node, StatefulAction, Status};

pub const NAME: &str = "behavior-tree";

/// Plain actions are `fn` pointers and cannot carry an `Op`; a stateful action can.
struct OpAction(Op);

impl StatefulAction<Bb> for OpAction {
    fn tick(&mut self, bb: &mut Bb) -> Status {
        match self.0.run(bb) {
            Outcome::Success => Status::Success,
            Outcome::Failure => Status::Failure,
            Outcome::Running => Status::Running,
        }
    }

    fn reset(&mut self) {}
}

fn build(spec: &Spec) -> Node<Bb> {
    match spec {
        Spec::Seq(children) => Node::sequence(children.iter().map(build).collect()),
        Spec::Sel(children) => Node::select(children.iter().map(build).collect()),
        Spec::Leaf(op) => Node::stateful_action("op", Box::new(OpAction(*op))),
        _ => unreachable!("only BASIC scenarios"),
    }
}

pub fn entries() -> Vec<Box<dyn Measure>> {
    // No custom composites: the enum of node kinds is closed.
    BASIC
        .into_iter()
        .map(|scenario| {
            // Nodes live in `Rc<RefCell<_>>` and hold their own state: one tree per agent.
            entry(
                NAME,
                false,
                scenario,
                move || scenario.spec(),
                |spec: &Spec| build(spec),
                |_, root, bb| match root.tick(1.0, bb) {
                    Status::Success => Outcome::Success,
                    Status::Failure => Outcome::Failure,
                    Status::Running => Outcome::Running,
                    Status::Initialized => unreachable!("tick never reports Initialized"),
                },
            )
        })
        .collect()
}
