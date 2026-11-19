//! Builders and blackboards sharing an act type apply structural changes correctly.
use bevy_app::{App, TaskPoolPlugin, Update};
use bevy_ecs::prelude::*;
use bevy_ecs::schedule::ScheduleLabel;
use flatbt_bevy::prelude::*;

#[derive(Component)]
struct Walker {
    active: bool,
    destination: u32,
}
#[derive(Component)]
struct Worker {
    active: bool,
    job: u32,
}
#[derive(Component, Debug, PartialEq)]
struct Order(u32);

fn walk() -> impl BehaviorNode<Walker, Order> {
    leaf(|walker: &mut Walker| {
        if walker.active {
            NodeResult::Running(Order(walker.destination))
        } else {
            NodeResult::Success
        }
    })
}
fn work() -> impl BehaviorNode<Worker, Order> {
    leaf(|worker: &mut Worker| {
        if worker.active {
            NodeResult::Running(Order(worker.job))
        } else {
            NodeResult::Success
        }
    })
}

#[derive(Resource, Default)]
struct Seen(Vec<u32>);
fn observe(orders: Query<&Order>, mut seen: ResMut<Seen>) {
    seen.0 = orders.iter().map(|order| order.0).collect();
    seen.0.sort_unstable();
}

#[test]
fn different_blackboards_share_an_act_type_across_serial_and_parallel_ticks() {
    for parallel in [false, true] {
        let mut app = App::new();
        app.add_plugins(TaskPoolPlugin::default());
        let worker_plugin = BehaviorPlugin::for_tree(work);
        app.add_plugins((
            BehaviorPlugin::for_tree(walk),
            if parallel {
                worker_plugin.parallel()
            } else {
                worker_plugin
            },
        ))
        .init_resource::<Seen>()
        .add_systems(Update, observe.after(BehaviorSystems));
        let walker = app
            .world_mut()
            .spawn((
                Walker {
                    active: true,
                    destination: 1,
                },
                Behavior::for_tree(walk),
            ))
            .id();
        let workers: Vec<_> = (10..74)
            .map(|job| {
                app.world_mut()
                    .spawn((Worker { active: true, job }, Behavior::for_tree(work)))
                    .id()
            })
            .collect();

        app.update();
        let expected: Vec<_> = core::iter::once(1).chain(10..74).collect();
        assert_eq!(app.world().resource::<Seen>().0, expected);

        // One tree removes its act while another keeps writing an existing act.
        app.world_mut().get_mut::<Walker>(walker).unwrap().active = false;
        app.world_mut().get_mut::<Worker>(workers[0]).unwrap().job = 100;
        app.update();
        let expected: Vec<_> = (11..74).chain(core::iter::once(100)).collect();
        assert_eq!(app.world().resource::<Seen>().0, expected);
        assert!(app.world().get::<Order>(walker).is_none());

        // The other direction: an act appears while the other tree's acts go.
        app.world_mut().get_mut::<Walker>(walker).unwrap().active = true;
        for worker in workers {
            app.world_mut().get_mut::<Worker>(worker).unwrap().active = false;
        }
        app.update();
        assert_eq!(app.world().resource::<Seen>().0, [1]);
    }
}

#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
struct WorkPhase;

#[test]
fn only_the_tree_in_the_running_schedule_changes_its_acts() {
    let mut app = App::new();
    app.add_plugins((
        BehaviorPlugin::for_tree(walk),
        BehaviorPlugin::for_tree(work).in_schedule(WorkPhase),
    ));
    let walker = app
        .world_mut()
        .spawn((
            Walker {
                active: true,
                destination: 1,
            },
            Behavior::for_tree(walk),
        ))
        .id();
    let worker = app
        .world_mut()
        .spawn((
            Worker {
                active: true,
                job: 2,
            },
            Behavior::for_tree(work),
        ))
        .id();

    app.update();
    assert_eq!(app.world().get::<Order>(walker), Some(&Order(1)));
    assert!(app.world().get::<Order>(worker).is_none());
    app.world_mut().run_schedule(WorkPhase);
    assert_eq!(app.world().get::<Order>(worker), Some(&Order(2)));

    app.world_mut().get_mut::<Walker>(walker).unwrap().active = false;
    app.world_mut().run_schedule(WorkPhase);
    assert_eq!(app.world().get::<Order>(walker), Some(&Order(1)));
    app.update();
    assert!(app.world().get::<Order>(walker).is_none());
    assert_eq!(app.world().get::<Order>(worker), Some(&Order(2)));

    app.world_mut().get_mut::<Worker>(worker).unwrap().active = false;
    app.update();
    assert_eq!(app.world().get::<Order>(worker), Some(&Order(2)));
    app.world_mut().run_schedule(WorkPhase);
    assert!(app.world().get::<Order>(worker).is_none());
}
