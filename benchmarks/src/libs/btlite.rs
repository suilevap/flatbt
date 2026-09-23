use crate::common::{Bb, Op, Outcome, SCENARIOS, Spec};
use crate::harness::{Measure, entry};
use behavior_tree_lite::{
    BehaviorCallback, BehaviorNode, BehaviorNodeContainer, BehaviorResult, Context, FallbackNode,
    SequenceNode,
};
use std::any::Any;
use std::cell::Cell;

pub const NAME: &str = "behavior-tree-lite";

/// Nodes reach non-`'static` agent data only through the tick callback. The
/// query carries its answer back in a `Cell`, so the callback returns `None`
/// and never boxes a reply.
struct Query {
    op: Op,
    out: Cell<Outcome>,
}

struct OpNode(Op);

impl BehaviorNode for OpNode {
    fn tick(&mut self, arg: BehaviorCallback, _: &mut Context) -> BehaviorResult {
        let query = Query {
            op: self.0,
            out: Cell::new(Outcome::Failure),
        };
        arg(&query);
        match query.out.get() {
            Outcome::Success => BehaviorResult::Success,
            Outcome::Failure => BehaviorResult::Fail,
            Outcome::Running => BehaviorResult::Running,
        }
    }
}

fn build(spec: &Spec) -> BehaviorNodeContainer {
    let (mut node, children) = match spec {
        Spec::Seq(children) => (
            BehaviorNodeContainer::new_node(SequenceNode::default()),
            children,
        ),
        Spec::Sel(children) => (
            BehaviorNodeContainer::new_node(FallbackNode::default()),
            children,
        ),
        Spec::Leaf(op) => return BehaviorNodeContainer::new_node(OpNode(*op)),
    };
    for child in children {
        node.add_child(build(child)).expect("unbounded children");
    }
    node
}

pub struct Agent {
    root: BehaviorNodeContainer,
    ctx: Context,
}

fn tick(agent: &mut Agent, bb: &mut Bb) -> Outcome {
    let mut callback = |value: &dyn Any| -> Option<Box<dyn Any>> {
        let query = value
            .downcast_ref::<Query>()
            .expect("only queries are sent");
        query.out.set(query.op.run(bb));
        None
    };
    match agent.root.tick(&mut callback, &mut agent.ctx) {
        BehaviorResult::Success => Outcome::Success,
        BehaviorResult::Fail => Outcome::Failure,
        BehaviorResult::Running => Outcome::Running,
    }
}

pub fn entries() -> Vec<Box<dyn Measure>> {
    SCENARIOS
        .into_iter()
        .map(|scenario| {
            // Containers are neither `Clone` nor shareable: each agent builds its own.
            entry(
                NAME,
                false,
                scenario,
                move || scenario.spec(),
                |spec: &Spec| Agent {
                    root: build(spec),
                    ctx: Context::default(),
                },
                |_, agent, bb| tick(agent, bb),
            )
        })
        .collect()
}
