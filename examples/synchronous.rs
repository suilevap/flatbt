//! Synchronous reasoning and a custom repeat policy. No gameplay acts are applied.

use flatbt::{BtState, update};
use flatbt::{EntryMode, NodeResult, check, control, leaf, select, seq};

mod support;
use support::Repeat;

#[derive(Clone, Copy, Default)]
struct Observed {
    enemy_visible: bool,
    ammo: usize,
    has_route: bool,
}

#[derive(Default)]
struct Blackboard {
    observed: Observed,
    thoughts: Vec<&'static str>,
}

fn main() {
    let tree = select((
        seq((
            check(|bb: &Blackboard| bb.observed.enemy_visible && bb.observed.ammo >= 2),
            leaf(|bb: &mut Blackboard| {
                bb.thoughts.push("estimate aim");
                NodeResult::Success
            }),
            control(
                Repeat(2),
                (leaf(|bb: &mut Blackboard| {
                    // Comparing a possible shot does not spend a real round.
                    bb.thoughts.push("score a possible shot");
                    NodeResult::Success
                }),),
            ),
        )),
        seq((
            check(|bb: &Blackboard| bb.observed.has_route),
            leaf(|bb: &mut Blackboard| {
                bb.thoughts.push("consider patrol route");
                NodeResult::Success
            }),
        )),
        leaf(|bb: &mut Blackboard| {
            bb.thoughts.push("no useful plan");
            NodeResult::Success
        }),
    ));

    for (name, observed, expected) in [
        (
            "combat",
            Observed {
                enemy_visible: true,
                ammo: 2,
                has_route: true,
            },
            vec![
                "estimate aim",
                "score a possible shot",
                "score a possible shot",
            ],
        ),
        (
            "patrol",
            Observed {
                has_route: true,
                ..Observed::default()
            },
            vec!["consider patrol route"],
        ),
        ("idle", Observed::default(), vec!["no useful plan"]),
    ] {
        let ammo_before_thinking = observed.ammo;
        let mut bb = Blackboard {
            observed,
            thoughts: Vec::new(),
        };
        let result = update(
            &tree,
            &mut BtState::<_, _>::new(&tree),
            &mut bb,
            EntryMode::Resume,
        );
        assert_eq!(result, NodeResult::Success);
        assert_eq!(bb.thoughts, expected);
        assert_eq!(bb.observed.ammo, ammo_before_thinking);
        println!("{name}: {result:?} | {}", bb.thoughts.join(" -> "));
    }
}
