//! Compares FlatBT with other Rust behavior tree crates. See README.md.

mod alloc;
mod common;
mod harness;
mod libs;

use common::{SCENARIOS, Scenario};
use harness::{Config, Measure, Report};
use std::process::ExitCode;

#[global_allocator]
static COUNTING: alloc::Counting = alloc::Counting;

fn all() -> Vec<Box<dyn Measure>> {
    [
        libs::flatbt::entries(),
        libs::bonsai::entries(),
        libs::behavior_tree::entries(),
        libs::bhv::entries(),
        libs::btlite::entries(),
    ]
    .into_iter()
    .flatten()
    .collect()
}

const USAGE: &str = "\
usage: flatbt-compare [--quick] [--lib NAME] [--scenario NAME]
       flatbt-compare ticks LIB SCENARIO N   (ticks one agent N times; for profilers)";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let entries = all();
    if args.first().map(String::as_str) == Some("ticks") {
        let [_, lib, scenario, n] = &args[..] else {
            eprintln!("{USAGE}");
            return ExitCode::FAILURE;
        };
        let Some(entry) = entries
            .iter()
            .find(|e| e.lib() == lib && Some(e.scenario()) == Scenario::parse(scenario))
        else {
            eprintln!("unknown library or scenario");
            return ExitCode::FAILURE;
        };
        let bb = entry.ticks(n.parse().expect("N is a tick count"));
        println!("{bb:?}");
        return ExitCode::SUCCESS;
    }

    let mut config = Config {
        single_ticks: 2_000_000,
        agents: 10_000,
        frames: 200,
        samples: 7,
    };
    let (mut lib, mut scenario) = (None, None);
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--quick" => {
                config.single_ticks = 200_000;
                config.frames = 20;
                config.samples = 3;
            }
            "--lib" => lib = args.next().cloned(),
            "--scenario" => scenario = args.next().and_then(|s| Scenario::parse(s)),
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::FAILURE;
            }
        }
    }

    println!("{}", baseline(&config));
    let mut ok = true;
    for s in SCENARIOS
        .into_iter()
        .filter(|s| scenario.is_none_or(|x| x == *s))
    {
        // Every library must behave identically to FlatBT on the same inputs.
        let reference = entries
            .iter()
            .find(|e| e.lib() == libs::flatbt::NAME && e.scenario() == s)
            .expect("flatbt covers every scenario")
            .trace(100_000);
        let reports: Vec<Report> = entries
            .iter()
            .filter(|e| e.scenario() == s && lib.as_deref().is_none_or(|l| l == e.lib()))
            .map(|e| {
                eprintln!("running {} / {}", e.lib(), s.name());
                e.run(&config)
            })
            .collect();
        ok &= print_table(s, &reports, &reference);
    }
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// What the harness itself costs per tick: the world step and the loop.
fn baseline(config: &Config) -> String {
    let world = harness::entry(
        "world",
        true,
        Scenario::Patrol,
        || (),
        |_| (),
        |_, _, _| common::Outcome::Running,
    );
    let r = world.run(config);
    format!(
        "World step alone (included in every ns/tick below): {:.1} ns/tick for 1 agent, {:.1} ns/tick for {}k agents.",
        r.ns_per_tick,
        r.ns_per_agent_tick,
        config.agents / 1000
    )
}

fn print_table(s: Scenario, reports: &[Report], reference: &(common::Bb, [u64; 3])) -> bool {
    let base = reports.iter().find(|r| r.lib == libs::flatbt::NAME);
    let ratio = |value: f64, of: fn(&Report) -> f64| match base {
        Some(b) if of(b) > 0.0 => format!(" ({:.1}×)", value / of(b)),
        _ => String::new(),
    };
    let [success, failure, running] = reference.1;
    println!(
        "\n### {} — {success} success / {failure} failure / {running} running per 100k ticks\n",
        s.name()
    );
    println!(
        "| library | ns/tick, 1 agent | ns/tick, 10k agents | allocs/tick | bytes/tick | bytes/agent (inline + heap) | allocs to build an agent | ns to build an agent | peak transient heap | shared tree | same result |"
    );
    println!("|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|:-:|");
    let mut ok = true;
    for r in reports {
        let same = r.checksum == *reference;
        ok &= same;
        println!(
            "| {} | {:.1}{} | {:.1}{} | {:.2} | {:.1} | {} + {:.0} | {:.1} | {:.0} | {} | {} | {} |",
            r.lib,
            r.ns_per_tick,
            ratio(r.ns_per_tick, |b| b.ns_per_tick),
            r.ns_per_agent_tick,
            ratio(r.ns_per_agent_tick, |b| b.ns_per_agent_tick),
            r.allocs_per_tick,
            r.bytes_per_tick,
            r.agent_inline,
            r.agent_heap,
            r.build_allocs,
            r.build_ns,
            r.transient_peak,
            r.shared.map_or("—".to_owned(), |b| b.to_string()),
            if same { "yes" } else { "**NO**" },
        );
        if !same {
            eprintln!("{}: {:?}\nflatbt: {:?}", r.lib, r.checksum, reference);
        }
    }
    ok
}
