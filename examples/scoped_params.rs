use flatbt::prelude::*;

#[path = "support/scoped_params.rs"]
mod support;
#[path = "support/wait_frames.rs"]
mod wait;
use support::*;

fn main() {
    fn get_visible_door_pos(world: &mut World) -> Vector2 {
        world.selections += 1;
        world.visible_door
    }
    let tree = scope! {
        let walk_pos: Vector2 = |world: &mut World| {
            world.selections += 1;
            world.next_patrol
        };
        let door_pos: Vector2 = get_visible_door_pos;
        sequence {
            LookAt.with(door_pos);
            wait::wait_frames(1);
            action(Walk).with(walk_pos);
        }
    };
    let mut state = BtState::new(&tree);
    let mut world = World {
        next_patrol: Vector2(10.0, 20.0),
        visible_door: Vector2(90.0, 80.0),
        looked_at: Vec::new(),
        walked_to: Vec::new(),
        selections: 0,
    };
    assert_eq!(
        update(&tree, &mut state, &mut world, EntryMode::Resume),
        NodeResult::Running
    );
    // Initializers ran once. Their values survive changes to the world.
    world.next_patrol = Vector2(-1.0, -1.0);
    assert_eq!(
        update(&tree, &mut state, &mut world, EntryMode::Evaluate),
        NodeResult::Running
    );
    assert_eq!(
        update(&tree, &mut state, &mut world, EntryMode::Resume),
        NodeResult::Success
    );
    assert_eq!(world.selections, 2);
    assert_eq!(world.looked_at, [Vector2(90.0, 80.0)]);
    assert_eq!(world.walked_to, [Vector2(10.0, 20.0)]);
    println!(
        "Looked at {:?}; walked to {:?}",
        world.looked_at, world.walked_to
    );
}
