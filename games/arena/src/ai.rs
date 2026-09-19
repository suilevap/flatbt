//! Three enemy minds over one blackboard.
//!
//! The blackboard is what a fighter knows; what it *decides* leaves as
//! components, one per standing action, and ordinary systems do the work by
//! matching them. No node here changes the world: each starts an action and
//! waits for the world to finish it.
//!
//! The gather is three systems at three rates, which is the shape a single
//! per-agent gather function could not express: cover has to be searched for,
//! so it is looked up once every ten ticks and only for agents that asked.

use core::time::Duration;

use bevy::prelude::*;
use flatbt::bevy::prelude::*;
use flatbt::prelude::{BtAction, Request, action, ask, choose};

use crate::world::{Ammo, Arena, Cover, Health, Speed};

/// How often a fighter is allowed to abandon what it is doing.
///
/// A tree reconsiders on its own whenever an invocation ends, so this is not
/// what makes it reactive. It is for the branch that does *not* end: walking to
/// cover takes a hundred frames, and a coward healed halfway there should turn
/// around. `evaluate_every` staggers the agents so the whole population does
/// not reconsider on one frame.
pub const RETHINK: Duration = Duration::from_millis(100);

// --- what a fighter is doing, as components ----------------------------------

/// Walking somewhere, and where. `movement` moves it there.
#[derive(Component, Debug, PartialEq)]
pub struct MovingTo(pub Vec2);

/// Shooting. `weapons` spends the rounds.
#[derive(Component, Debug, Default, PartialEq)]
pub struct Firing;

/// Swinging. `weapons` charges for it.
#[derive(Component, Debug, Default, PartialEq)]
pub struct Meleeing;

/// Refilling the magazine. `weapons` decides how long that takes.
#[derive(Component, Debug, Default, PartialEq)]
pub struct Reloading;

/// Looking for somewhere to hide. `find_cover` answers it.
#[derive(Component, Debug, Default, PartialEq)]
pub struct LookingForCover;

/// Everything but the trees: the gather, the five components a decision can
/// become, and the systems that act on them. Split out so the bench can put
/// other trees over the same world.
pub fn support(app: &mut App) {
    // `ACTIONS=move` registers only the one whose value changes without the
    // component coming and going, so the bench can separate the cost of a sync
    // pass from the cost of an archetype move. `ACTIONS=none` registers none.
    match std::env::var("ACTIONS").unwrap_or_default().as_str() {
        "none" => {}
        "move" => {
            app.add_plugins(ActionComponent::describing(|f: &Fighter| {
                f.move_to.map(MovingTo)
            }));
        }
        _ => {
            app.add_plugins((
                ActionComponent::describing(|f: &Fighter| f.move_to.map(MovingTo)),
                ActionComponent::<_, Firing>::while_(|f: &Fighter| f.firing),
                ActionComponent::<_, Meleeing>::while_(|f: &Fighter| f.meleeing),
                ActionComponent::<_, Reloading>::while_(|f: &Fighter| f.reloading),
                ActionComponent::<_, LookingForCover>::while_(|f: &Fighter| f.cover.is_pending()),
            ));
        }
    }
    app.add_systems(
        Update,
        (gather_agent, gather_pace, find_cover).before(BehaviorSystems),
    )
    .add_systems(Update, (movement, weapons).after(ActionSystems));
}

/// [`support`] plus the three minds, ticked across the task pool.
///
/// `.parallel()` is the whole opt-in: a tree touches one component, its own
/// blackboard, which is disjoint per entity.
pub fn plugin(app: &mut App) {
    app.add_plugins(support).add_plugins((
        BehaviorPlugin::for_tree(chaser).tick_mode(pace).parallel(),
        BehaviorPlugin::for_tree(sniper).tick_mode(pace).parallel(),
        BehaviorPlugin::for_tree(coward).tick_mode(pace).parallel(),
    ));
}

// --- the blackboard ----------------------------------------------------------

/// What a fighter knows, and what it wants.
///
/// The bottom half is not an interface: every field there is carried out to a
/// component by `plugin` above, and nothing outside this module reads it.
#[derive(Component, Default, Debug)]
pub struct Fighter {
    // What the world said.
    pub position: Vec2,
    pub health: f32,
    pub ammo: u32,
    pub speed: f32,
    pub player: Vec2,
    /// Whether this agent's staggered slot to reconsider falls in this tick.
    pub rethink: bool,
    /// How much of a reload the world still has to do. `weapons` owns it.
    pub reload_left: u32,
    // What it decided.
    pub move_to: Option<Vec2>,
    pub firing: bool,
    pub meleeing: bool,
    pub reloading: bool,
    /// The question and its answer. `Pending` becomes [`LookingForCover`].
    pub cover: Request<Vec2>,
}

/// What this tick does with one agent.
///
/// Both non-`Evaluate` branches are optimisations: answering `Tick::Evaluate`
/// every tick is always correct and only makes fighters quicker to change their
/// minds. `Skip` pays off because a fighter walking to a spot it already chose
/// has nothing to decide until it arrives -- `movement` is doing the walking.
pub fn pace(fighter: &Fighter) -> Tick {
    if walking(fighter) && !skipping_is_off() {
        Tick::Skip
    } else if fighter.rethink {
        Tick::Evaluate
    } else {
        Tick::Resume
    }
}

/// `NOSKIP=1` turns [`Tick::Skip`] off, so the bench can measure the same
/// population both ways in one run. Read once; the answer cannot change.
fn skipping_is_off() -> bool {
    use std::sync::OnceLock;
    static OFF: OnceLock<bool> = OnceLock::new();
    *OFF.get_or_init(|| std::env::var_os("NOSKIP").is_some())
}

/// Every tick: the cheap half, straight off the agent's own components.
///
/// Decisions are not cleared here. They stand until a tree revises them, which
/// is what makes them components worth matching on rather than per-tick flags.
pub fn gather_agent(
    mut agents: Query<(&Transform, &Health, &Ammo, &Speed, &mut Fighter)>,
    arena: Res<Arena>,
) {
    agents
        .par_iter_mut()
        .for_each(|(transform, health, ammo, speed, mut fighter)| {
            let fighter = fighter.bypass_change_detection();
            fighter.position = transform.translation.truncate();
            fighter.health = health.0;
            fighter.ammo = ammo.0;
            fighter.speed = speed.0;
            fighter.player = arena.player;
            fighter.reload_left = 6u32.saturating_sub(ammo.0);
        });
}

/// Every tenth tick, and only for agents that asked: the expensive half.
///
/// A stand-in for a raycast or a path query, and the reason the gather is the
/// game's: a single per-agent gather function has nowhere to put "this field,
/// at a tenth of the rate, for these agents only". Here that is a `Local`, and
/// the agents are named by a component the tree put there.
pub fn find_cover(
    mut asking: Query<(&Transform, &mut Fighter), With<LookingForCover>>,
    cover: Query<&Transform, With<Cover>>,
    arena: Res<Arena>,
    mut next: Local<u32>,
) {
    if arena.tick < *next {
        return;
    }
    *next = arena.tick + 10;
    let spots: Vec<Vec2> = cover.iter().map(|t| t.translation.truncate()).collect();
    for (transform, mut fighter) in asking.iter_mut() {
        let from = transform.translation.truncate();
        let nearest = spots.iter().copied().min_by(|a, b| {
            a.distance_squared(from)
                .total_cmp(&b.distance_squared(from))
        });
        let fighter = fighter.bypass_change_detection();
        match nearest {
            Some(spot) => fighter.cover.answer(spot),
            None => fighter.cover.clear(),
        }
    }
}

/// And the pace, which is neither of the above.
pub fn gather_pace(mut agents: Query<(Entity, &mut Fighter)>, arena: Res<Arena>) {
    agents.par_iter_mut().for_each(|(entity, mut fighter)| {
        fighter.bypass_change_detection().rethink =
            evaluate_every(RETHINK, arena.elapsed, arena.delta, entity) == Tick::Evaluate;
    });
}

// --- the three minds ---------------------------------------------------------
//
// Nothing below knows the ECS exists. `&mut Fighter` is all a node sees, and
// `tests/trees.rs` runs these with no `World`.

fn range_to_player(f: &Fighter) -> f32 {
    f.position.distance(f.player)
}

/// Still on its way. Read off the world rather than off a flag: `movement`
/// moves the fighter, and this is how a tree notices it arrived.
fn walking(f: &Fighter) -> bool {
    f.move_to.is_some_and(|at| f.position.distance(at) > 8.0)
}

/// Walks somewhere and waits until it is there.
struct MoveTo {
    where_to: fn(&Fighter) -> Vec2,
}

impl BtAction<Fighter> for MoveTo {
    type State = ();

    fn start(&self, f: &mut Fighter, _: ()) -> Option<()> {
        f.move_to = Some((self.where_to)(f));
        Some(())
    }

    fn is_in_progress(&self, _: &(), f: &Fighter, _: ()) -> bool {
        walking(f)
    }

    fn complete(&self, _: &mut (), f: &mut Fighter, _: ()) -> bool {
        f.move_to = None;
        true
    }
}

/// Swings while the player is in reach. `weapons` charges for it.
struct Melee;

impl BtAction<Fighter> for Melee {
    type State = ();

    fn start(&self, f: &mut Fighter, _: ()) -> Option<()> {
        f.meleeing = true;
        Some(())
    }

    fn is_in_progress(&self, _: &(), f: &Fighter, _: ()) -> bool {
        range_to_player(f) < 24.0
    }

    fn complete(&self, _: &mut (), f: &mut Fighter, _: ()) -> bool {
        f.meleeing = false;
        true
    }
}

pub fn chaser() -> impl BehaviorNode<Fighter> {
    select((
        seq((
            check(|f: &Fighter| range_to_player(f) < 24.0),
            action(Melee),
        )),
        action(MoveTo {
            where_to: |f: &Fighter| f.player,
        }),
    ))
}

/// Signals a reload and waits. The tree does not know how long it takes or how
/// many rounds it puts back; `weapons` answers by working `reload_left` down.
struct Reload;

impl BtAction<Fighter> for Reload {
    type State = ();

    fn start(&self, f: &mut Fighter, _: ()) -> Option<()> {
        f.reloading = true;
        Some(())
    }

    fn is_in_progress(&self, _: &(), f: &Fighter, _: ()) -> bool {
        f.reload_left > 0
    }

    fn complete(&self, _: &mut (), f: &mut Fighter, _: ()) -> bool {
        f.reloading = false;
        true
    }
}

/// Fires while in range with rounds left. `weapons` spends them.
struct Fire;

impl BtAction<Fighter> for Fire {
    type State = ();

    fn start(&self, f: &mut Fighter, _: ()) -> Option<()> {
        f.firing = true;
        Some(())
    }

    fn is_in_progress(&self, _: &(), f: &Fighter, _: ()) -> bool {
        range_to_player(f) < 420.0 && f.ammo > 0
    }

    fn complete(&self, _: &mut (), f: &mut Fighter, _: ()) -> bool {
        f.firing = false;
        true
    }
}

pub fn sniper() -> impl BehaviorNode<Fighter> {
    select((
        seq((check(|f: &Fighter| f.ammo == 0), action(Reload))),
        seq((
            check(|f: &Fighter| range_to_player(f) < 180.0),
            action(MoveTo {
                where_to: |f: &Fighter| f.position * 2.0 - f.player,
            }),
        )),
        seq((
            check(|f: &Fighter| range_to_player(f) < 420.0 && f.ammo > 0),
            action(Fire),
        )),
        action(MoveTo {
            where_to: |f: &Fighter| f.player,
        }),
    ))
}

/// The one thing a tree cannot do for itself: find the nearest cover.
///
/// `ask` puts the question once per invocation; it becomes `LookingForCover`,
/// `find_cover` matches that and answers, and the answer arrives as a plain
/// `Vec2` in a scope local -- so `MoveTo` cannot run without one, and a coward
/// that heals and comes back asks again rather than walking to a spot picked
/// for an older situation.
struct WalkToSpot;

impl BtNode<Fighter, &Vec2> for WalkToSpot {
    type State = ();

    fn update(&self, _: &mut (), f: &mut Fighter, spot: &Vec2, _: EntryMode) -> NodeResult {
        if f.position.distance(*spot) < 8.0 {
            f.move_to = None;
            return NodeResult::Success;
        }
        f.move_to = Some(*spot);
        NodeResult::Running
    }
}

fn take_cover() -> impl BehaviorNode<Fighter> {
    flatbt::scope::scope! {
        let spot: Vec2;
        sequence {
            ask(
                |f: &mut Fighter| f.cover.ask(),
                |f: &Fighter| f.cover.answered().copied(),
            ).with(out spot);
            WalkToSpot.with(spot);
        }
    }
}

pub fn coward() -> impl BehaviorNode<Fighter> {
    choose!(|f: &Fighter| match f.health < 40.0 {
        true => take_cover(),
        false => chaser(),
    })
}

// --- the systems that do the work --------------------------------------------
//
// Each matches a component a tree put there, and none of them mentions the
// tree, the blackboard, or FlatBT.

pub fn movement(mut agents: Query<(&MovingTo, &Speed, &mut Transform)>) {
    agents
        .par_iter_mut()
        .for_each(|(target, speed, mut transform)| {
            let from = transform.translation.truncate();
            let step = (target.0 - from).normalize_or_zero() * speed.0;
            if step != Vec2::ZERO {
                transform.translation += step.extend(0.0);
            }
        });
}

/// Every number here -- what a shot costs, how full a magazine is, what a swing
/// does -- belongs to the game, and no tree above ever saw it.
pub fn weapons(
    mut firing: Query<&mut Ammo, With<Firing>>,
    mut reloading: Query<&mut Ammo, (With<Reloading>, Without<Firing>)>,
    mut meleeing: Query<&mut Health, With<Meleeing>>,
) {
    for mut ammo in firing.iter_mut() {
        if ammo.0 > 0 {
            ammo.0 -= 1;
        }
    }
    for mut ammo in reloading.iter_mut() {
        ammo.set_if_neq(Ammo((ammo.0 + 1).min(6)));
    }
    for mut health in meleeing.iter_mut() {
        health.0 -= 0.05;
    }
}
