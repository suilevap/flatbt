use crate::common::{Bb, Op, Order, Outcome, SCENARIOS, Spec};
use crate::harness::{Measure, entry};
use behavior_tree_lite::{
    BehaviorCallback, BehaviorNode, BehaviorNodeContainer, BehaviorResult, Context, FallbackNode,
    NumChildren, Registry, SequenceNode,
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

/// A condition read by a custom node.
struct Ask {
    condition: fn(&Bb) -> bool,
    out: Cell<bool>,
}

/// The next child an ordered node visits.
struct Pick {
    order: Order,
    used: u64,
    count: usize,
    out: Cell<Option<usize>>,
}

fn ask(arg: BehaviorCallback, condition: fn(&Bb) -> bool) -> bool {
    let ask = Ask {
        condition,
        out: Cell::new(false),
    };
    arg(&ask);
    ask.out.get()
}

fn pick(arg: BehaviorCallback, order: Order, used: u64, count: usize) -> Option<usize> {
    let pick = Pick {
        order,
        used,
        count,
        out: Cell::new(None),
    };
    arg(&pick);
    pick.out.get()
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

fn one(child: &Spec) -> Vec<&Spec> {
    vec![child]
}

fn build(spec: &Spec) -> BehaviorNodeContainer {
    use BehaviorNodeContainer as N;
    let (mut node, children): (N, Vec<&Spec>) = match spec {
        Spec::Seq(children) => (
            N::new_node(SequenceNode::default()),
            children.iter().collect(),
        ),
        Spec::Sel(children) => (
            N::new_node(FallbackNode::default()),
            children.iter().collect(),
        ),
        Spec::Leaf(op) => return N::new_node(OpNode(*op)),
        // Its decorators are reachable only through the registry that text
        // trees are built from.
        Spec::Invert(child) => (N::new_raw(builtin("Inverter")), one(child)),
        Spec::ForceSuccess(child) => (N::new_raw(builtin("ForceSuccess")), one(child)),
        // Its `Repeat` and `Retry` read their count from a blackboard port, and
        // it has no ordered node: these are written against `BehaviorNode`.
        Spec::Ordered(select, order, children) => (
            N::new_node(Ordered {
                select: *select,
                order: *order,
                used: 0,
                position: 0,
                running: None,
            }),
            children.iter().collect(),
        ),
        Spec::Repeat(times, child) => (N::new_node(Repeat::new(*times, false)), one(child)),
        Spec::Retry(attempts, child) => (N::new_node(Repeat::new(*attempts, true)), one(child)),
        Spec::IfElse(condition, then, otherwise) => (
            N::new_node(IfElse {
                condition: *condition,
                running: None,
            }),
            vec![&**then, &**otherwise],
        ),
        Spec::RepeatWhile(condition, child) => (
            N::new_node(RepeatWhile {
                condition: *condition,
                ran: false,
            }),
            one(child),
        ),
    };
    for child in children {
        node.add_child(build(child))
            .expect("within the node's child limit");
    }
    node
}

fn builtin(name: &str) -> Box<dyn BehaviorNode> {
    Registry::default().build(name).expect("a built-in node")
}

fn status(result: Option<BehaviorResult>) -> BehaviorResult {
    result.expect("the child exists")
}

/// `select` or `seq` over children in the order `order` picks.
struct Ordered {
    select: bool,
    order: Order,
    used: u64,
    position: usize,
    running: Option<usize>,
}

impl Ordered {
    fn finish(&mut self, result: BehaviorResult) -> BehaviorResult {
        self.used = 0;
        self.position = 0;
        self.running = None;
        result
    }
}

impl BehaviorNode for Ordered {
    fn tick(&mut self, arg: BehaviorCallback, ctx: &mut Context) -> BehaviorResult {
        let count = ctx.num_children();
        let (done, next) = if self.select {
            (BehaviorResult::Success, BehaviorResult::Fail)
        } else {
            (BehaviorResult::Fail, BehaviorResult::Success)
        };
        let mut child = match self.running.take() {
            Some(child) => child,
            None => match pick(arg, self.order, 0, count) {
                Some(child) => child,
                None => return self.finish(BehaviorResult::Fail),
            },
        };
        loop {
            let result = status(ctx.tick_child(child, arg));
            if result == BehaviorResult::Running {
                self.running = Some(child);
                return result;
            }
            if result == done {
                return self.finish(result);
            }
            self.used |= 1 << child;
            self.position += 1;
            if self.position == count {
                return self.finish(next);
            }
            child = match pick(arg, self.order, self.used, count) {
                Some(child) => child,
                None => return self.finish(BehaviorResult::Fail),
            };
        }
    }

    fn max_children(&self) -> NumChildren {
        NumChildren::Finite(64)
    }
}

/// `repeat` (until `times` successes) or `retry` (until one success in at
/// most `times` attempts).
struct Repeat {
    times: usize,
    retry: bool,
    count: usize,
    running: bool,
}

impl Repeat {
    fn new(times: usize, retry: bool) -> Self {
        Self {
            times,
            retry,
            count: 0,
            running: false,
        }
    }
}

impl BehaviorNode for Repeat {
    fn tick(&mut self, arg: BehaviorCallback, ctx: &mut Context) -> BehaviorResult {
        let (again, last) = if self.retry {
            (BehaviorResult::Fail, BehaviorResult::Fail)
        } else {
            (BehaviorResult::Success, BehaviorResult::Success)
        };
        if !self.running {
            if self.times == 0 {
                return last;
            }
            self.count = 0;
        }
        loop {
            let result = status(ctx.tick_child(0, arg));
            if result == BehaviorResult::Running {
                self.running = true;
                return result;
            }
            self.running = false;
            if result != again {
                return result;
            }
            self.count += 1;
            if self.count == self.times {
                return last;
            }
        }
    }

    fn max_children(&self) -> NumChildren {
        NumChildren::Finite(1)
    }
}

struct IfElse {
    condition: fn(&Bb) -> bool,
    running: Option<usize>,
}

impl BehaviorNode for IfElse {
    fn tick(&mut self, arg: BehaviorCallback, ctx: &mut Context) -> BehaviorResult {
        let branch = match self.running.take() {
            Some(branch) => branch,
            None if ask(arg, self.condition) => 0,
            None => 1,
        };
        let result = status(ctx.tick_child(branch, arg));
        if result == BehaviorResult::Running {
            self.running = Some(branch);
        }
        result
    }

    fn max_children(&self) -> NumChildren {
        NumChildren::Finite(2)
    }
}

/// FlatBT's `repeat_while`: the condition is asked on every update and after
/// each run; a run that never returned `Running` fails the loop.
struct RepeatWhile {
    condition: fn(&Bb) -> bool,
    ran: bool,
}

impl BehaviorNode for RepeatWhile {
    fn tick(&mut self, arg: BehaviorCallback, ctx: &mut Context) -> BehaviorResult {
        if !ask(arg, self.condition) {
            self.ran = false;
            return BehaviorResult::Success;
        }
        loop {
            let result = status(ctx.tick_child(0, arg));
            if result == BehaviorResult::Running {
                self.ran = true;
                return result;
            }
            let ran = core::mem::take(&mut self.ran);
            if !ask(arg, self.condition) {
                return BehaviorResult::Success;
            }
            if !(result == BehaviorResult::Success && ran) {
                return BehaviorResult::Fail;
            }
        }
    }

    fn max_children(&self) -> NumChildren {
        NumChildren::Finite(1)
    }
}

pub struct Agent {
    root: BehaviorNodeContainer,
    ctx: Context,
}

fn tick(agent: &mut Agent, bb: &mut Bb) -> Outcome {
    let mut callback = |value: &dyn Any| -> Option<Box<dyn Any>> {
        if let Some(query) = value.downcast_ref::<Query>() {
            query.out.set(query.op.run(bb));
        } else if let Some(ask) = value.downcast_ref::<Ask>() {
            ask.out.set((ask.condition)(bb));
        } else if let Some(pick) = value.downcast_ref::<Pick>() {
            pick.out.set(pick.order.next(bb, pick.used, pick.count));
        }
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
