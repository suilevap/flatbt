//! Time nodes over a blackboard that holds `Time::elapsed`, with their memory
//! kept in the `Behavior` component.

use std::time::Duration;

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use flatbt_bevy::prelude::*;

#[derive(Component, Default)]
struct Runner {
    elapsed: Duration,
}

impl BtClock for Runner {
    type Instant = Duration;
    type Duration = Duration;
    fn now(&self) -> Duration {
        self.elapsed
    }
}

#[derive(Component, Clone, Copy, PartialEq, Debug)]
enum Act {
    Dash,
    Walk,
}

fn runner() -> impl BehaviorNode<Runner, Act> {
    select((
        cooldown(
            Duration::from_secs(10),
            action_wait(Duration::from_secs(1), |_: &Runner| Act::Dash),
        ),
        action_while(|_: &Runner| true, |_: &Runner| Act::Walk),
    ))
}

#[test]
fn a_cooldown_outlives_its_branch_in_the_component() {
    let mut app = App::new();
    app.add_plugins(BehaviorPlugin::for_tree(runner));
    let agent = app
        .world_mut()
        .spawn((Runner::default(), Behavior::for_tree(runner)))
        .id();
    let at = |app: &mut App, secs: u64| {
        app.world_mut().get_mut::<Runner>(agent).unwrap().elapsed = Duration::from_secs(secs);
        app.update();
        app.world().get::<Act>(agent).copied()
    };

    assert_eq!(at(&mut app, 0), Some(Act::Dash));
    // The dash ends: the tree succeeds and decides nothing this tick.
    assert_eq!(at(&mut app, 1), None);
    assert_eq!(at(&mut app, 2), Some(Act::Walk));
    assert_eq!(at(&mut app, 9), Some(Act::Walk));
    assert_eq!(at(&mut app, 10), Some(Act::Dash));
}
