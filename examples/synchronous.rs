use flatbt::{BtState, update};
use flatbt::{EntryMode, NodeResult, check, control, leaf, select, seq};

mod support;
use support::Repeat;

#[derive(Default)]
struct Agent {
    enemy_visible: bool,
    ammo: usize,
    has_route: bool,
    trace: Vec<&'static str>,
}

fn main() {
    let tree = select((
        seq((
            check(|ctx: &Agent| ctx.enemy_visible && ctx.ammo >= 2),
            leaf(|ctx: &mut Agent| {
                ctx.trace.push("aim");
                NodeResult::Success
            }),
            control(
                Repeat(2),
                (leaf(|ctx: &mut Agent| {
                    ctx.ammo -= 1;
                    ctx.trace.push("fire");
                    NodeResult::Success
                }),),
            ),
        )),
        seq((
            check(|ctx: &Agent| ctx.has_route),
            leaf(|ctx: &mut Agent| {
                ctx.trace.push("patrol");
                NodeResult::Success
            }),
        )),
        leaf(|ctx: &mut Agent| {
            ctx.trace.push("idle");
            NodeResult::Success
        }),
    ));

    for (name, mut agent, expected) in [
        (
            "combat",
            Agent {
                enemy_visible: true,
                ammo: 2,
                has_route: true,
                ..Agent::default()
            },
            vec!["aim", "fire", "fire"],
        ),
        (
            "patrol",
            Agent {
                has_route: true,
                ..Agent::default()
            },
            vec!["patrol"],
        ),
        ("idle", Agent::default(), vec!["idle"]),
    ] {
        let result = update(
            &tree,
            &mut BtState::<_, _>::new(&tree),
            &mut agent,
            EntryMode::Resume,
        );
        assert_eq!(result, NodeResult::Success);
        assert_eq!(agent.trace, expected);
        println!("{name}: {result:?} | {}", agent.trace.join(" -> "));
    }
}
