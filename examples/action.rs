use flatbt::{BtAction, BtState, EntryMode, NodeResult, action, seq, update};

#[derive(Default)]
struct Agent {
    effects: Vec<&'static str>,
}

struct Task {
    name: &'static str,
    steps: usize,
}

// Invocation data can require initialization through start rather than Default.
struct Progress(usize);

impl BtAction<Agent> for Task {
    type State = Progress;

    fn start(&self, _: &mut Agent, _: ()) -> Option<Progress> {
        Some(Progress(0))
    }

    fn is_in_progress(&self, state: &Progress, _: &Agent, _: ()) -> bool {
        state.0 < self.steps
    }

    fn tick(&self, state: &mut Progress, ctx: &mut Agent, _: ()) {
        state.0 += 1;
        ctx.effects.push(self.name);
    }
}

fn main() {
    let root = seq((
        action(Task {
            name: "move",
            steps: 2,
        }),
        action(Task {
            name: "fire",
            steps: 1,
        }),
    ));
    let mut state = BtState::new(&root);
    let mut agent = Agent::default();
    for frame in 1..=4 {
        let result = update(&root, &mut state, &mut agent, EntryMode::Resume);
        println!("update {frame}: {result:?} | {:?}", agent.effects);
        if frame == 3 {
            // Move completes and Fire starts and ticks in this same update.
            assert_eq!(result, NodeResult::Running);
            assert_eq!(agent.effects, ["move", "move", "fire"]);
        }
        if frame == 4 {
            assert_eq!(result, NodeResult::Success);
        }
    }
    assert!(!state.is_running());
}
