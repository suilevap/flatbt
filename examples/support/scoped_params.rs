use flatbt::{BtAction, BtNode, Entry, NodeResult};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vector2(pub f32, pub f32);

/// Facts supplied by the application, read while planning.
#[derive(Clone, Copy)]
pub struct Observed {
    pub next_patrol: Vector2,
    pub visible_door: Vector2,
}

/// No gameplay state: these writes record considered views and planned routes.
pub struct Blackboard {
    pub observed: Observed,
    pub considered_views: Vec<Vector2>,
    pub planned_routes: Vec<Vector2>,
    pub selections: usize,
}

pub struct ConsiderView;
impl BtNode<Blackboard, (), &Vector2> for ConsiderView {
    type State = ();
    type Memory = ();

    fn update(
        &self,
        _: &mut (),
        _: &mut (),
        bb: &mut Blackboard,
        position: &Vector2,
        _: Entry<'_>,
    ) -> NodeResult {
        bb.considered_views.push(*position);
        NodeResult::Success
    }
}

/// A toy route calculation. Completing it does not move the agent.
pub struct PlanRoute;
impl BtAction<Blackboard, (), &Vector2> for PlanRoute {
    type State = usize;

    fn start(&self, _: &mut Blackboard, _: &Vector2) -> Option<usize> {
        Some(0)
    }
    fn is_in_progress(&self, ticks: &usize, _: &Blackboard, _: &Vector2) -> bool {
        *ticks < 1
    }
    fn tick(&self, ticks: &mut usize, bb: &mut Blackboard, destination: &Vector2) {
        bb.planned_routes.push(*destination);
        *ticks += 1;
    }
}
