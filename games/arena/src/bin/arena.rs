//! Many enemies, three minds, one blackboard -- on screen.
//!
//! Everything is a coloured rectangle: the point is the AI cost, so nothing is
//! spent on art. The readout separates the behaviour ticks from the rest of the
//! frame, which is the number this crate exists to watch.
//!
//! ```sh
//! cargo run --release --bin arena
//! cargo run --release --bin arena -- 50000   # start with this many enemies
//! ```

use std::time::{Duration, Instant};

use arena::Mind;
use arena::ai::{chaser, coward, sniper};
use arena::world::{
    ARENA, Ammo, Arena, Cover, CoverTarget, Health, Player, Speed, resolve_cover_requests,
    track_arena,
};
use bevy::prelude::*;
use flatbt::bevy::prelude::*;

const START: u32 = 20_000;
const STEP: u32 = 5_000;
const PLAYER_SPEED: f32 = 260.0;

/// Wall time spent inside [`BehaviorSystems`], averaged over the last second.
#[derive(Resource, Default)]
struct AiCost {
    started: Option<Instant>,
    window: Duration,
    frames: u32,
    shown: f64,
}

/// How many enemies exist, so the readout need not count them.
#[derive(Resource, Default)]
struct Population(u32);

/// Where the next enemy goes, so repeated spawns do not pile up.
#[derive(Resource, Default)]
struct SpawnCursor(u32);

#[derive(Component)]
struct Readout;

fn main() {
    let start = std::env::args()
        .nth(1)
        .and_then(|n| n.parse().ok())
        .unwrap_or(START);

    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "FlatBT arena".into(),
                resolution: (1280u32, 800u32).into(),
                present_mode: bevy::window::PresentMode::AutoNoVsync,
                ..default()
            }),
            ..default()
        }))
        .init_resource::<Arena>()
        .init_resource::<AiCost>()
        .insert_resource(Population(start))
        .init_resource::<SpawnCursor>()
        // One registration per tree. `.parallel()` is the whole opt-in to
        // spreading agents across the task pool.
        .add_plugins((
            BehaviorPlugin::for_tree(chaser).parallel(),
            BehaviorPlugin::for_tree(sniper).parallel(),
            BehaviorPlugin::for_tree(coward).parallel(),
        ))
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                // Everything the trees read is written before they run.
                (move_player, track_arena).chain().before(BehaviorSystems),
                start_timing.before(BehaviorSystems),
                stop_timing.after(BehaviorSystems),
                // ...and everything they asked for is answered after.
                (resolve_cover_requests, recover_in_cover, adjust_population).after(stop_timing),
                readout.after(stop_timing),
            ),
        )
        .run();
}

fn setup(mut commands: Commands, population: Res<Population>, mut cursor: ResMut<SpawnCursor>) {
    commands.spawn((
        Camera2d,
        Projection::from(OrthographicProjection {
            scale: 1.6,
            ..OrthographicProjection::default_2d()
        }),
    ));

    commands.spawn((
        Player,
        Transform::default(),
        Sprite::from_color(Color::WHITE, Vec2::splat(14.0)),
    ));

    for index in 0..24 {
        commands.spawn((
            Cover,
            Transform::from_translation(scatter(index * 7919).extend(-1.0)),
            Sprite::from_color(Color::srgb(0.22, 0.24, 0.28), Vec2::splat(46.0)),
        ));
    }

    commands.spawn((
        Readout,
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(15.0),
            ..default()
        },
        Node {
            position_type: PositionType::Absolute,
            top: px(10),
            left: px(10),
            ..default()
        },
    ));

    for _ in 0..population.0 {
        spawn_enemy(&mut commands, cursor.0);
        cursor.0 += 1;
    }
}

/// A cheap deterministic spread, so a run looks the same twice.
fn scatter(index: u32) -> Vec2 {
    let hash = index.wrapping_mul(2_654_435_761);
    let x = (hash >> 16) as f32 / 65_536.0;
    let y = (hash & 0xffff) as f32 / 65_536.0;
    Vec2::new(x - 0.5, y - 0.5) * ARENA
}

fn spawn_enemy(commands: &mut Commands, index: u32) {
    let mind = Mind::nth(index);
    let (color, size) = match mind {
        Mind::Chaser => (Color::srgb(0.85, 0.25, 0.25), 7.0),
        Mind::Sniper => (Color::srgb(0.35, 0.55, 0.95), 6.0),
        Mind::Coward => (Color::srgb(0.95, 0.80, 0.30), 6.0),
    };
    let body = (
        Transform::from_translation(scatter(index).extend(0.0)),
        Sprite::from_color(color, Vec2::splat(size)),
        Health(100.0 - (index % 90) as f32),
        Ammo(index % 7),
        Speed(60.0 + (index % 3) as f32 * 25.0),
    );
    // `Behavior::for_tree` names the tree by the same function the plugin was
    // given: spelling them the same way is the whole registration.
    match mind {
        Mind::Chaser => commands.spawn((body, Behavior::for_tree(chaser))),
        Mind::Sniper => commands.spawn((body, Behavior::for_tree(sniper))),
        Mind::Coward => commands.spawn((body, Behavior::for_tree(coward))),
    };
}

fn move_player(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut player: Query<&mut Transform, With<Player>>,
) {
    let mut dir = Vec2::ZERO;
    for (key, delta) in [
        (KeyCode::KeyW, Vec2::Y),
        (KeyCode::ArrowUp, Vec2::Y),
        (KeyCode::KeyS, Vec2::NEG_Y),
        (KeyCode::ArrowDown, Vec2::NEG_Y),
        (KeyCode::KeyA, Vec2::NEG_X),
        (KeyCode::ArrowLeft, Vec2::NEG_X),
        (KeyCode::KeyD, Vec2::X),
        (KeyCode::ArrowRight, Vec2::X),
    ] {
        if keys.pressed(key) {
            dir += delta;
        }
    }
    let Ok(mut transform) = player.single_mut() else {
        return;
    };
    let step = dir.normalize_or_zero() * PLAYER_SPEED * time.delta_secs();
    transform.translation += step.extend(0.0);
    transform.translation = transform
        .translation
        .clamp(Vec3::splat(-ARENA), Vec3::splat(ARENA));
}

fn start_timing(mut cost: ResMut<AiCost>) {
    cost.started = Some(Instant::now());
}

fn stop_timing(mut cost: ResMut<AiCost>) {
    let Some(started) = cost.started.take() else {
        return;
    };
    cost.window += started.elapsed();
    cost.frames += 1;
    if cost.frames >= 30 {
        cost.shown = cost.window.as_secs_f64() * 1000.0 / cost.frames as f64;
        cost.window = Duration::ZERO;
        cost.frames = 0;
    }
}

/// Cowards that reached their spot heal, and leave once they are well again.
///
/// Dropping [`CoverTarget`] is what sends them back through the asking branch,
/// so the deferred query runs more than once per agent over a session.
fn recover_in_cover(
    time: Res<Time>,
    mut hiding: Query<(Entity, &Transform, &mut Health, &CoverTarget)>,
    mut commands: Commands,
) {
    for (entity, transform, mut health, target) in hiding.iter_mut() {
        if transform.translation.truncate().distance(target.0) > 10.0 {
            continue;
        }
        health.0 = (health.0 + 25.0 * time.delta_secs()).min(100.0);
        if health.0 >= 80.0 {
            commands.entity(entity).remove::<CoverTarget>();
        }
    }
}

fn adjust_population(
    keys: Res<ButtonInput<KeyCode>>,
    enemies: Query<Entity, With<Health>>,
    mut population: ResMut<Population>,
    mut cursor: ResMut<SpawnCursor>,
    mut commands: Commands,
) {
    if keys.just_pressed(KeyCode::BracketRight) {
        for _ in 0..STEP {
            spawn_enemy(&mut commands, cursor.0);
            cursor.0 += 1;
        }
        population.0 += STEP;
    }
    if keys.just_pressed(KeyCode::BracketLeft) {
        let going = STEP.min(population.0);
        for entity in enemies.iter().take(going as usize) {
            commands.entity(entity).despawn();
        }
        population.0 -= going;
    }
}

fn readout(
    cost: Res<AiCost>,
    population: Res<Population>,
    time: Res<Time>,
    mut text: Query<&mut Text, With<Readout>>,
) {
    let Ok(mut text) = text.single_mut() else {
        return;
    };
    let frame = time.delta_secs_f64() * 1000.0;
    let share = if frame > 0.0 {
        cost.shown / frame * 100.0
    } else {
        0.0
    };
    **text = format!(
        "enemies {}\nAI {:.2} ms ({share:.0}% of frame)\nframe {frame:.1} ms\n\nWASD move   [ ] fewer/more enemies",
        population.0, cost.shown,
    );
}
