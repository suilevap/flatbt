use NodeResult::{Failure, Running, Success};
use flatbt::scope::{Read, WithParams, Write, bind, no_params, params, read, scope, write};
use flatbt::{BtNode, BtState, EntryMode, NodeResult, action, leaf, select, seq, update};

#[path = "../examples/support/scoped_params.rs"]
mod support;
#[path = "../examples/support/wait_frames.rs"]
mod wait;
use support::*;

#[derive(Default)]
struct PatrolLocals {
    walk_pos: Option<Vector2>,
    door_pos: Option<Vector2>,
}

fn world() -> World {
    World {
        next_patrol: Vector2(1.0, 2.0),
        visible_door: Vector2(8.0, 9.0),
        looked_at: Vec::new(),
        walked_to: Vec::new(),
        selections: 0,
    }
}

#[test]
fn named_bindings_distinguish_equal_types_and_survive_producer_and_consumer_suspension() {
    let tree = scope::<PatrolLocals, _>(seq((
        bind(
            GetNextPatrolPos,
            write(|s: &mut PatrolLocals| &mut s.walk_pos),
        ),
        bind(
            GetVisibleDoorPos,
            write(|s: &mut PatrolLocals| &mut s.door_pos),
        ),
        LookAt.with(read(|s: &PatrolLocals| s.door_pos.as_ref())),
        no_params(wait::wait_frames(1)),
        action(Walk).with(read(|s: &PatrolLocals| s.walk_pos.as_ref())),
    )));
    let mut first = BtState::new(&tree);
    let mut second = BtState::new(&tree);
    let mut world = world();
    assert_eq!(
        update(&tree, &mut first, &mut world, EntryMode::Resume),
        Running
    );
    world.next_patrol = Vector2(3.0, 4.0);
    assert_eq!(
        update(&tree, &mut second, &mut world, EntryMode::Resume),
        Running
    );
    for state in [&mut first, &mut second] {
        assert_eq!(
            update(&tree, state, &mut world, EntryMode::Evaluate),
            Running
        );
        assert_eq!(update(&tree, state, &mut world, EntryMode::Resume), Running);
        assert_eq!(update(&tree, state, &mut world, EntryMode::Resume), Success);
    }
    assert_eq!(world.looked_at, [world.visible_door, world.visible_door]);
    assert_eq!(world.walked_to, [Vector2(1.0, 2.0), Vector2(3.0, 4.0)]);
    assert_eq!(world.selections, 4);
    assert_eq!(
        update(&tree, &mut first, &mut world, EntryMode::Resume),
        Running
    );
    assert_eq!(world.selections, 5);
}

#[test]
fn the_same_node_accepts_a_different_field_and_an_unrelated_scope_layout() {
    #[derive(Default)]
    struct Other {
        destination: Option<Vector2>,
        unused: usize,
    }
    let tree = seq((
        scope::<PatrolLocals, _>(seq((
            bind(
                GetNextPatrolPos,
                write(|s: &mut PatrolLocals| &mut s.walk_pos),
            ),
            bind(LookAt, read(|s: &PatrolLocals| s.walk_pos.as_ref())),
        ))),
        scope::<Other, _>(seq((
            bind(GetNextPatrolPos, write(|s: &mut Other| &mut s.destination)),
            bind(
                LookAt,
                read(|s: &Other| {
                    let _ = s.unused;
                    s.destination.as_ref()
                }),
            ),
        ))),
    ));
    let mut state = BtState::new(&tree);
    let mut world = world();
    assert_eq!(
        update(&tree, &mut state, &mut world, EntryMode::Resume),
        Success
    );
    assert_eq!(world.looked_at, [world.next_patrol, world.next_patrol]);
}

#[test]
fn a_node_can_request_an_input_and_a_different_output_of_the_same_type() {
    struct Offset;
    impl BtNode<World, (&Vector2, &mut Option<Vector2>)> for Offset {
        type State = ();
        fn update(
            &self,
            _: &mut (),
            _: &mut World,
            (input, output): (&Vector2, &mut Option<Vector2>),
            _: EntryMode,
        ) -> NodeResult {
            *output = Some(Vector2(input.0 + 5.0, input.1 + 6.0));
            Success
        }
    }
    let tree = scope::<PatrolLocals, _>(seq((
        bind(
            GetNextPatrolPos,
            write(|s: &mut PatrolLocals| &mut s.walk_pos),
        ),
        bind(
            Offset,
            params::<(Read<Vector2>, Write<Option<Vector2>>), _, _>(|s: &mut PatrolLocals| {
                Some((s.walk_pos.as_ref()?, &mut s.door_pos))
            }),
        ),
        bind(LookAt, read(|s: &PatrolLocals| s.door_pos.as_ref())),
    )));
    let mut state = BtState::new(&tree);
    let mut world = world();
    assert_eq!(
        update(&tree, &mut state, &mut world, EntryMode::Resume),
        Success
    );
    assert_eq!(world.looked_at, [Vector2(6.0, 8.0)]);
}

#[test]
fn missing_input_fails_the_branch_without_calling_the_consumer() {
    let tree = select((
        scope::<PatrolLocals, _>(bind(LookAt, read(|s: &PatrolLocals| s.door_pos.as_ref()))),
        leaf(|world: &mut World| {
            world.selections += 1;
            Success
        }),
    ));
    let mut state = BtState::new(&tree);
    let mut world = world();
    assert_eq!(
        update(&tree, &mut state, &mut world, EntryMode::Resume),
        Success
    );
    assert!(world.looked_at.is_empty());
    assert_eq!(world.selections, 1);
}

#[test]
fn failed_candidates_keep_writes_to_the_shared_enclosing_scope() {
    struct Increment;
    impl BtNode<(), &mut usize> for Increment {
        type State = ();
        fn update(&self, _: &mut (), _: &mut (), value: &mut usize, _: EntryMode) -> NodeResult {
            *value += 1;
            Failure
        }
    }
    struct AssertTwo;
    impl BtNode<(), &usize> for AssertTwo {
        type State = ();
        fn update(&self, _: &mut (), _: &mut (), value: &usize, _: EntryMode) -> NodeResult {
            assert_eq!(*value, 2);
            Success
        }
    }
    let tree = scope::<usize, _>(select((
        bind(Increment, write(|n: &mut usize| n)),
        seq((
            no_params(wait::wait_frames(1)),
            bind(AssertTwo, read(|n: &usize| Some(n))),
        )),
    )));
    let mut state = BtState::new(&tree);
    assert_eq!(
        update(&tree, &mut state, &mut (), EntryMode::Resume),
        Running
    );
    assert_eq!(
        update(&tree, &mut state, &mut (), EntryMode::Evaluate),
        Success
    );
}

#[test]
fn macro_binds_ordered_inputs_and_outputs_and_keeps_definition_captures() {
    struct Offset(f32);
    impl BtNode<World, (&Vector2, &mut Option<Vector2>)> for Offset {
        type State = ();
        fn update(
            &self,
            _: &mut (),
            _: &mut World,
            (input, output): (&Vector2, &mut Option<Vector2>),
            _: EntryMode,
        ) -> NodeResult {
            *output = Some(Vector2(input.0 + self.0, input.1));
            Success
        }
    }
    struct Compare;
    impl BtNode<World, (&Vector2, &Vector2)> for Compare {
        type State = ();
        fn update(
            &self,
            _: &mut (),
            _: &mut World,
            (a, b): (&Vector2, &Vector2),
            _: EntryMode,
        ) -> NodeResult {
            assert_eq!(b.0 - a.0, 10.0);
            Success
        }
    }
    let constructed = std::cell::Cell::new(0);
    let offset = 10.0;
    let tree = scope! {
        let walk_pos: Vector2 = { constructed.set(constructed.get() + 1); |world: &mut World| world.next_patrol };
        let door_pos: Vector2;
        sequence {
            Offset(offset).with(walk_pos, out door_pos);
            Compare.with(walk_pos, door_pos);
            LookAt.with(door_pos);
            wait::wait_frames(1);
            action(Walk).with(walk_pos);
        }
    };
    let mut state = BtState::new(&tree);
    let mut world = world();
    assert_eq!(
        update(&tree, &mut state, &mut world, EntryMode::Resume),
        Running
    );
    assert_eq!(
        update(&tree, &mut state, &mut world, EntryMode::Evaluate),
        Running
    );
    assert_eq!(
        update(&tree, &mut state, &mut world, EntryMode::Resume),
        Success
    );
    assert_eq!(constructed.get(), 1);
    assert_eq!(world.looked_at, [Vector2(11.0, 2.0)]);
    assert_eq!(world.walked_to, [Vector2(1.0, 2.0)]);
}

#[test]
fn nested_branch_scopes_release_children_before_locals_on_preemption_reset_and_completion() {
    use std::sync::{Arc, Mutex};
    type Log = Arc<Mutex<Vec<&'static str>>>;
    struct Resource(Log, &'static str);
    impl Drop for Resource {
        fn drop(&mut self) {
            self.0.lock().unwrap().push(self.1);
        }
    }
    struct Context {
        urgent: bool,
        done: bool,
        log: Log,
    }
    struct Pending;
    impl BtNode<Context, &Resource> for Pending {
        type State = Option<Resource>;
        fn update(
            &self,
            state: &mut Self::State,
            ctx: &mut Context,
            _: &Resource,
            _: EntryMode,
        ) -> NodeResult {
            state.get_or_insert_with(|| Resource(ctx.log.clone(), "child"));
            if ctx.done { Success } else { Running }
        }
    }
    let tree = scope! {
        let outer: Resource = |ctx: &mut Context| Resource(ctx.log.clone(), "outer");
        select {
            scope! {
                let local: Resource = |ctx: &mut Context| Resource(ctx.log.clone(), "candidate");
                sequence {
                    flatbt::check(|ctx: &Context| ctx.urgent);
                    Pending.with(local);
                }
            };
            scope! {
                let local: Resource = |ctx: &mut Context| Resource(ctx.log.clone(), "saved");
                sequence { Pending.with(local); }
            };
        }
    };
    let log = Log::default();
    let mut ctx = Context {
        urgent: false,
        done: false,
        log: log.clone(),
    };
    let mut state = BtState::new(&tree);
    assert_eq!(
        update(&tree, &mut state, &mut ctx, EntryMode::Resume),
        Running
    );
    assert_eq!(
        update(&tree, &mut state, &mut ctx, EntryMode::Evaluate),
        Running
    );
    assert_eq!(*log.lock().unwrap(), ["candidate", "candidate"]);
    ctx.urgent = true;
    assert_eq!(
        update(&tree, &mut state, &mut ctx, EntryMode::Evaluate),
        Running
    );
    assert_eq!(
        *log.lock().unwrap(),
        ["candidate", "candidate", "child", "saved"]
    );
    state.reset();
    assert_eq!(
        *log.lock().unwrap(),
        [
            "candidate",
            "candidate",
            "child",
            "saved",
            "child",
            "candidate",
            "outer"
        ]
    );
    log.lock().unwrap().clear();
    assert_eq!(
        update(&tree, &mut state, &mut ctx, EntryMode::Resume),
        Running
    );
    ctx.done = true;
    assert_eq!(
        update(&tree, &mut state, &mut ctx, EntryMode::Resume),
        Success
    );
    assert_eq!(*log.lock().unwrap(), ["child", "candidate", "outer"]);
    log.lock().unwrap().clear();
    ctx.done = false;
    assert_eq!(
        update(&tree, &mut state, &mut ctx, EntryMode::Resume),
        Running
    );
    drop(state);
    assert_eq!(*log.lock().unwrap(), ["child", "candidate", "outer"]);
}

#[test]
fn bindings_support_more_than_three_ordered_inputs_and_outputs() {
    struct Combine;
    impl
        BtNode<
            World,
            (
                &Vector2,
                &Vector2,
                &Vector2,
                &mut Option<Vector2>,
                &mut Option<Vector2>,
            ),
        > for Combine
    {
        type State = ();

        fn update(
            &self,
            _: &mut (),
            _: &mut World,
            (a, b, c, first, second): (
                &Vector2,
                &Vector2,
                &Vector2,
                &mut Option<Vector2>,
                &mut Option<Vector2>,
            ),
            _: EntryMode,
        ) -> NodeResult {
            *first = Some(Vector2(a.0, b.1));
            *second = Some(Vector2(b.0 + c.0, a.1 + c.1));
            Success
        }
    }

    let tree = scope! {
        let walk_pos: Vector2 = |world: &mut World| world.next_patrol;
        let door_pos: Vector2;
        let first: Vector2;
        let second: Vector2;
        sequence {
            GetVisibleDoorPos.with(out door_pos);
            Combine.with(door_pos, walk_pos, door_pos, out first, out second);
            LookAt.with(first);
            LookAt.with(second);
        }
    };
    let mut state = BtState::new(&tree);
    let mut world = world();
    assert_eq!(
        update(&tree, &mut state, &mut world, EntryMode::Resume),
        Running
    );
    assert_eq!(
        update(&tree, &mut state, &mut world, EntryMode::Resume),
        Success
    );
    assert_eq!(world.looked_at, [Vector2(8.0, 2.0), Vector2(9.0, 18.0)]);
}

struct GetNextPatrolPos;
impl BtNode<World, &mut Option<Vector2>> for GetNextPatrolPos {
    type State = ();

    fn update(
        &self,
        _: &mut (),
        world: &mut World,
        output: &mut Option<Vector2>,
        _: EntryMode,
    ) -> NodeResult {
        *output = Some(world.next_patrol);
        world.selections += 1;
        NodeResult::Success
    }
}

struct GetVisibleDoorPos;
impl BtNode<World, &mut Option<Vector2>> for GetVisibleDoorPos {
    type State = bool;

    fn update(
        &self,
        started: &mut bool,
        world: &mut World,
        output: &mut Option<Vector2>,
        _: EntryMode,
    ) -> NodeResult {
        // Demonstrate a producer whose computation takes more than one update.
        if !*started {
            *started = true;
            return NodeResult::Running;
        }
        *output = Some(world.visible_door);
        world.selections += 1;
        NodeResult::Success
    }
}

#[test]
fn selector_revalidates_children_without_recomputing_function_locals() {
    struct Context {
        next: u32,
        prefer_first: bool,
        init_order: Vec<&'static str>,
        visits: Vec<(u32, EntryMode, usize)>,
    }
    fn first(ctx: &mut Context) -> u32 {
        ctx.init_order.push("first");
        ctx.next
    }
    struct Visit;
    impl BtNode<Context, &u32> for Visit {
        type State = usize;
        fn update(
            &self,
            count: &mut usize,
            ctx: &mut Context,
            input: &u32,
            mode: EntryMode,
        ) -> NodeResult {
            *count += 1;
            ctx.visits.push((*input, mode, *count));
            if *count < 3 { Running } else { Success }
        }
    }
    let tree = scope! {
        context: Context;
        let a: u32 = first;
        let b: u32 = |ctx| {
            ctx.init_order.push("second");
            ctx.next + 10
        };
        select {
            sequence {
                flatbt::check(|ctx: &Context| ctx.prefer_first);
                Visit.with(a);
            }
            sequence { Visit.with(b); }
        }
    };
    let mut state = BtState::new(&tree);
    let mut ctx = Context {
        next: 1,
        prefer_first: false,
        init_order: Vec::new(),
        visits: Vec::new(),
    };
    assert!(ctx.init_order.is_empty());
    assert_eq!(
        update(&tree, &mut state, &mut ctx, EntryMode::Resume),
        Running
    );
    ctx.next = 100;
    ctx.prefer_first = true;
    assert_eq!(
        update(&tree, &mut state, &mut ctx, EntryMode::Resume),
        Running
    );
    assert_eq!(
        update(&tree, &mut state, &mut ctx, EntryMode::Evaluate),
        Running
    );
    assert_eq!(
        update(&tree, &mut state, &mut ctx, EntryMode::Resume),
        Running
    );
    assert_eq!(
        update(&tree, &mut state, &mut ctx, EntryMode::Resume),
        Success
    );
    assert_eq!(ctx.init_order, ["first", "second"]);
    assert_eq!(
        ctx.visits,
        [
            (11, EntryMode::Evaluate, 1),
            (11, EntryMode::Resume, 2),
            (1, EntryMode::Evaluate, 1),
            (1, EntryMode::Resume, 2),
            (1, EntryMode::Resume, 3),
        ]
    );
    assert_eq!(
        update(&tree, &mut state, &mut ctx, EntryMode::Resume),
        Running
    );
    assert_eq!(ctx.init_order, ["first", "second", "first", "second"]);
    assert_eq!(ctx.visits.last(), Some(&(100, EntryMode::Evaluate, 1)));
}

#[test]
fn action_callbacks_reborrow_mixed_parameters_without_copying_the_output() {
    struct Double;
    impl flatbt::BtAction<Vec<u32>, (&u32, &mut Option<u32>)> for Double {
        type State = bool;
        fn start(
            &self,
            _: &mut Vec<u32>,
            (input, output): (&u32, &mut Option<u32>),
        ) -> Option<bool> {
            *output = Some(*input);
            Some(false)
        }
        fn is_in_progress(&self, done: &bool, _: &Vec<u32>, _: (&u32, &mut Option<u32>)) -> bool {
            !done
        }
        fn tick(
            &self,
            done: &mut bool,
            _: &mut Vec<u32>,
            (input, output): (&u32, &mut Option<u32>),
        ) {
            *output = Some(*input * 2);
            *done = true;
        }
        fn complete(
            &self,
            _: &mut bool,
            trace: &mut Vec<u32>,
            (_, output): (&u32, &mut Option<u32>),
        ) -> bool {
            trace.push(output.unwrap());
            true
        }
    }
    let tree = scope! {
        context: Vec<u32>;
        let input: u32 = |_| 7;
        let output: u32;
        sequence { action(Double).with(input, out output); }
    };
    let mut state = BtState::new(&tree);
    let mut trace = Vec::new();
    assert_eq!(
        update(&tree, &mut state, &mut trace, EntryMode::Resume),
        Running
    );
    assert_eq!(
        update(&tree, &mut state, &mut trace, EntryMode::Resume),
        Success
    );
    assert_eq!(trace, [14]);
}

#[test]
fn scope_keeps_constructor_arguments_and_node_expressions_as_ordinary_rust() {
    struct Mark {
        position: Vector2,
    }
    impl BtNode<World> for Mark {
        type State = ();
        fn update(&self, _: &mut (), world: &mut World, _: (), _: EntryMode) -> NodeResult {
            world.looked_at.push(self.position);
            Success
        }
    }
    let frames = 1;
    let constructions = std::cell::Cell::new(0);
    let tree = scope! {
        context: World;
        let frames: Vector2 = |world| world.next_patrol;
        sequence {
            Mark { position: Vector2(5.0, 6.0) };
            {
                constructions.set(constructions.get() + 1);
                wait::wait_frames(frames)
            };
            support::LookAt.with(frames);
            leaf(|world: &mut World| {
                world.selections += 1;
                Success
            });
        }
    };
    assert_eq!(constructions.get(), 1);
    let mut state = BtState::new(&tree);
    let mut world = world();
    assert_eq!(
        update(&tree, &mut state, &mut world, EntryMode::Resume),
        Running
    );
    assert_eq!(
        update(&tree, &mut state, &mut world, EntryMode::Resume),
        Success
    );
    assert_eq!(world.looked_at, [Vector2(5.0, 6.0), world.next_patrol]);
    assert_eq!(world.selections, 1);
    assert_eq!(constructions.get(), 1);
}
