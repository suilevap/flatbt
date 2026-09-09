use flatbt::{BtAction, BtNode, EntryMode, NodeResult};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vector2(pub f32, pub f32);

pub struct World {
    pub next_patrol: Vector2,
    pub visible_door: Vector2,
    pub looked_at: Vec<Vector2>,
    pub walked_to: Vec<Vector2>,
    pub selections: usize,
}

pub struct LookAt;
impl BtNode<World, &Vector2> for LookAt {
    type State = ();

    fn update(
        &self,
        _: &mut (),
        world: &mut World,
        position: &Vector2,
        _: EntryMode,
    ) -> NodeResult {
        world.looked_at.push(*position);
        NodeResult::Success
    }
}

pub struct Walk;
impl BtAction<World, &Vector2> for Walk {
    type State = usize;

    fn start(&self, _: &mut World, _: &Vector2) -> Option<usize> {
        Some(0)
    }

    fn is_in_progress(&self, ticks: &usize, _: &World, _: &Vector2) -> bool {
        *ticks < 1
    }

    fn tick(&self, ticks: &mut usize, world: &mut World, destination: &Vector2) {
        world.walked_to.push(*destination);
        *ticks += 1;
    }
}
