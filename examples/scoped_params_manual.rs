//! Invocation-local planning inputs. Blackboard holds observations and reasoning;
//! this example prepares a route without moving anything in the world.

use flatbt::prelude::*;

#[path = "support/scoped_params.rs"]
mod support;
#[path = "support/wait_frames.rs"]
mod wait;
use support::*;

fn main() {
    fn get_visible_door_pos(bb: &mut Blackboard) -> Vector2 {
        bb.selections += 1;
        bb.observed.visible_door
    }
    // scope! generates these fields in the companion example.
    #[derive(Default)]
    struct Locals {
        walk_pos: Option<Vector2>,
        door_pos: Option<Vector2>,
    }

    let tree = scope::<Locals, _>(seq((
        // Run initializers once before the body.
        bind(
            compute(|bb: &mut Blackboard| {
                bb.selections += 1;
                bb.observed.next_patrol
            }),
            write(|locals: &mut Locals| &mut locals.walk_pos),
        ),
        bind(
            compute(get_visible_door_pos),
            write(|locals: &mut Locals| &mut locals.door_pos),
        ),
        // Scope body.
        seq((
            bind(
                ConsiderView,
                read(|locals: &Locals| locals.door_pos.as_ref()),
            ),
            no_params(wait::wait_frames(1)),
            bind(
                action(PlanRoute),
                read(|locals: &Locals| locals.walk_pos.as_ref()),
            ),
        )),
    )));
    let mut state: BtState<_, _> = BtState::new(&tree);
    let mut bb = Blackboard {
        observed: Observed {
            next_patrol: Vector2(10.0, 20.0),
            visible_door: Vector2(90.0, 80.0),
        },
        considered_views: Vec::new(),
        planned_routes: Vec::new(),
        selections: 0,
    };
    assert_eq!(
        update(&tree, &mut state, &mut bb, EntryMode::Resume),
        NodeResult::RUNNING
    );
    // The application refreshes perception. Already-bound planning inputs persist.
    bb.observed.next_patrol = Vector2(-1.0, -1.0);
    assert_eq!(
        update(&tree, &mut state, &mut bb, EntryMode::Evaluate),
        NodeResult::RUNNING
    );
    assert_eq!(
        update(&tree, &mut state, &mut bb, EntryMode::Resume),
        NodeResult::Success
    );
    assert_eq!(bb.selections, 2);
    assert_eq!(bb.considered_views, [Vector2(90.0, 80.0)]);
    assert_eq!(bb.planned_routes, [Vector2(10.0, 20.0)]);
    println!(
        "Considered view {:?}; planned route to {:?}",
        bb.considered_views, bb.planned_routes
    );
}
