use crate::common::{Bb, Op, Order, Outcome, SCENARIOS, Spec};
use crate::harness::{Measure, entry};
use bhv::{Bhv, BhvExt, Sel, Seq, Status};

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

/// bhv implements `Bhv` for concrete nodes only; this forwards to a boxed one
/// so its decorators (`inv`, `pass`) apply.
struct Boxed(Tree);

impl Bhv for Boxed {
    type Context = Bb;

    fn update(&mut self, bb: &mut Bb) -> Status {
        self.0.update(bb)
    }

    fn reset(&mut self, status: Status) {
        self.0.reset(status)
    }
}

fn build(spec: &Spec) -> Tree {
    let children = |specs: &[Spec]| specs.iter().map(build).collect::<Vec<_>>();
    match spec {
        Spec::Seq(specs) => Box::new(Seq::with_nodes(children(specs))),
        Spec::Sel(specs) => Box::new(Sel::with_nodes(children(specs))),
        Spec::Leaf(op) => Box::new(OpNode(*op)),
        // bhv has no ordered, retry or if-else node, and its `Repeat` counts
        // differently: these are written against its `Bhv` trait, as a user
        // of the crate would.
        Spec::Ordered(select, order, specs) => Box::new(Ordered {
            select: *select,
            order: *order,
            children: children(specs),
            used: 0,
            position: 0,
            running: None,
        }),
        Spec::Repeat(times, child) => Box::new(Repeat {
            times: *times,
            retry: false,
            child: build(child),
            count: 0,
            running: false,
        }),
        Spec::Retry(attempts, child) => Box::new(Repeat {
            times: *attempts,
            retry: true,
            child: build(child),
            count: 0,
            running: false,
        }),
        Spec::IfElse(condition, then, otherwise) => Box::new(IfElse {
            condition: *condition,
            branches: [build(then), build(otherwise)],
            running: None,
        }),
        Spec::Invert(child) => Box::new(Boxed(build(child)).inv()),
        Spec::ForceSuccess(child) => Box::new(Boxed(build(child)).pass()),
        Spec::RepeatWhile(condition, child) => Box::new(RepeatWhile {
            condition: *condition,
            child: build(child),
            ran: false,
        }),
    }
}

/// `select` or `seq` over children in the order `order` picks.
struct Ordered {
    select: bool,
    order: Order,
    children: Vec<Tree>,
    used: u64,
    position: usize,
    running: Option<usize>,
}

impl Ordered {
    fn finish(&mut self, status: Status) -> Status {
        self.used = 0;
        self.position = 0;
        self.running = None;
        status
    }
}

impl Bhv for Ordered {
    type Context = Bb;

    fn update(&mut self, bb: &mut Bb) -> Status {
        let count = self.children.len();
        let (done, next) = if self.select {
            (Status::Success, Status::Failure)
        } else {
            (Status::Failure, Status::Success)
        };
        let mut child = match self.running.take() {
            Some(child) => child,
            None => match self.order.next(bb, 0, count) {
                Some(child) => child,
                None => return self.finish(Status::Failure),
            },
        };
        loop {
            let status = self.children[child].update(bb);
            if status == Status::Running {
                self.running = Some(child);
                return status;
            }
            if status == done {
                return self.finish(status);
            }
            self.used |= 1 << child;
            self.position += 1;
            if self.position == count {
                return self.finish(next);
            }
            child = match self.order.next(bb, self.used, count) {
                Some(child) => child,
                None => return self.finish(Status::Failure),
            };
        }
    }
}

/// `repeat` (runs until `times` successes) or `retry` (until one success in
/// at most `times` attempts).
struct Repeat {
    times: usize,
    retry: bool,
    child: Tree,
    count: usize,
    running: bool,
}

impl Bhv for Repeat {
    type Context = Bb;

    fn update(&mut self, bb: &mut Bb) -> Status {
        let (again, last) = if self.retry {
            (Status::Failure, Status::Failure)
        } else {
            (Status::Success, Status::Success)
        };
        if !self.running {
            if self.times == 0 {
                return last;
            }
            self.count = 0;
        }
        loop {
            let status = self.child.update(bb);
            if status == Status::Running {
                self.running = true;
                return status;
            }
            self.running = false;
            if status != again {
                return status;
            }
            self.count += 1;
            if self.count == self.times {
                return last;
            }
        }
    }
}

struct IfElse {
    condition: fn(&Bb) -> bool,
    branches: [Tree; 2],
    running: Option<usize>,
}

impl Bhv for IfElse {
    type Context = Bb;

    fn update(&mut self, bb: &mut Bb) -> Status {
        let branch = self
            .running
            .take()
            .unwrap_or(if (self.condition)(bb) { 0 } else { 1 });
        let status = self.branches[branch].update(bb);
        if status == Status::Running {
            self.running = Some(branch);
        }
        status
    }
}

/// FlatBT's `repeat_while`: the condition is asked on every update and after
/// each run; a run that never returned `Running` fails the loop.
struct RepeatWhile {
    condition: fn(&Bb) -> bool,
    child: Tree,
    ran: bool,
}

impl Bhv for RepeatWhile {
    type Context = Bb;

    fn update(&mut self, bb: &mut Bb) -> Status {
        if !(self.condition)(bb) {
            self.ran = false;
            return Status::Success;
        }
        loop {
            let status = self.child.update(bb);
            if status == Status::Running {
                self.ran = true;
                return status;
            }
            let ran = core::mem::take(&mut self.ran);
            if !(self.condition)(bb) {
                return Status::Success;
            }
            if !(status == Status::Success && ran) {
                return Status::Failure;
            }
        }
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
