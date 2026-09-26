//! Compares FlatBT with other Rust behavior tree crates. See README.md.

mod alloc;
mod common;
mod harness;
mod libs;
mod soldier;
mod villager;

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
       flatbt-compare ticks LIB SCENARIO N   (ticks one agent N times; for profilers)
       flatbt-compare evaluate [--quick]     (flatbt with Evaluate, as TSV; see csharp/)
       flatbt-compare scaling [--quick]      (1 to 1M agents, as TSV; see charts/)
       flatbt-compare memory                 (heap at 1 to 1M agents, as TSV; see charts/)";

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

    let quick = args.iter().any(|a| a == "--quick");
    if args.first().map(String::as_str) == Some("memory") {
        memory(&entries);
        return ExitCode::SUCCESS;
    }
    if args.first().map(String::as_str) == Some("scaling") {
        scaling(&entries, quick);
        return ExitCode::SUCCESS;
    }
    if args.first().map(String::as_str) == Some("evaluate") {
        evaluate(&config(quick));
        return ExitCode::SUCCESS;
    }

    let config = config(quick);
    let (mut lib, mut scenario) = (None, None);
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--quick" => {}
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
        // Skipped without the `catalog` feature.
        let Some(reference) = entries
            .iter()
            .find(|e| e.lib() == libs::flatbt::NAME && e.scenario() == s)
        else {
            continue;
        };
        let reference = reference.trace(100_000);
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

fn config(quick: bool) -> Config {
    if quick {
        Config {
            single_ticks: 200_000,
            agents: 10_000,
            frames: 20,
            samples: 3,
        }
    } else {
        Config {
            single_ticks: 2_000_000,
            agents: 10_000,
            frames: 200,
            samples: 7,
        }
    }
}

/// Population sizes for `scaling`. A size is skipped when its agents would
/// hold more than `HEAP_LIMIT` bytes.
const AGENTS: [usize; 7] = [1, 10, 100, 1_000, 10_000, 100_000, 1_000_000];
const HEAP_LIMIT: f64 = 3e9;

/// Tab-separated rows for `charts/plot.py`: scenario, lib, agents, ns per
/// agent-tick, bytes per agent.
fn scaling(entries: &[Box<dyn Measure>], quick: bool) {
    let (agent_ticks, samples) = if quick { (200_000, 3) } else { (2_000_000, 5) };
    println!("scenario\tlib\tagents\tns_per_agent_tick\tbytes_per_agent");
    for e in entries {
        let per_agent = e.scaling(100, 1_000, 1).bytes_per_agent;
        for agents in AGENTS {
            if agents as f64 * per_agent > HEAP_LIMIT {
                eprintln!(
                    "skipping {} / {} at {agents} agents",
                    e.lib(),
                    e.scenario().name()
                );
                continue;
            }
            eprintln!(
                "running {} / {} at {agents} agents",
                e.lib(),
                e.scenario().name()
            );
            let r = e.scaling(agents, agent_ticks, samples);
            println!(
                "{}\t{}\t{agents}\t{:.2}\t{:.0}",
                e.scenario().name(),
                e.lib(),
                r.ns_per_agent_tick,
                r.bytes_per_agent
            );
        }
    }
}

/// Tab-separated rows for `charts/plot.py`: scenario, lib, agents, bytes held
/// by all agents' trees, allocations to build one agent, allocations and bytes
/// allocated per agent-tick, and peak heap growth while ticking.
fn memory(entries: &[Box<dyn Measure>]) {
    println!(
        "scenario\tlib\tagents\tagents_bytes\tbuild_allocs_per_agent\tallocs_per_agent_tick\tbytes_allocated_per_agent_tick\tpeak_growth"
    );
    for e in entries {
        let per_agent = e.scaling(100, 1_000, 1).bytes_per_agent;
        for agents in AGENTS {
            if agents as f64 * per_agent > HEAP_LIMIT {
                eprintln!(
                    "skipping {} / {} at {agents} agents",
                    e.lib(),
                    e.scenario().name()
                );
                continue;
            }
            eprintln!(
                "running {} / {} at {agents} agents",
                e.lib(),
                e.scenario().name()
            );
            // At least 200k agent-ticks, so rare allocations still show; a
            // large population ticks only a few frames, so its agents are
            // early in their runs. Steady-state rates come from the small ones.
            let m = e.memory(agents, (200_000 / agents).max(3));
            println!(
                "{}\t{}\t{agents}\t{:.0}\t{:.2}\t{:.4}\t{:.2}\t{}",
                e.scenario().name(),
                e.lib(),
                m.agents_bytes,
                m.build_allocs_per_agent,
                m.allocs_per_agent_tick,
                m.bytes_allocated_per_agent_tick,
                m.peak_growth
            );
        }
    }
}

/// Tab-separated rows in the format `csharp/Program.cs` prints, for
/// `csharp/compare.sh`: lib, scenario, ns/tick for 1 agent and for 10k,
/// bytes allocated per tick, bytes per agent, ns to build an agent, checksum.
fn evaluate(config: &Config) {
    for e in libs::flatbt::evaluate_entries()
        .into_iter()
        .filter(|e| common::BASIC.contains(&e.scenario()))
    {
        eprintln!("running {} / {}", e.lib(), e.scenario().name());
        let r = e.run(config);
        println!(
            "{}\t{}\t{:.1}\t{:.1}\t{:.1}\t{:.0}\t{:.0}\t{}",
            e.lib(),
            e.scenario().name(),
            r.ns_per_tick,
            r.ns_per_agent_tick,
            r.bytes_per_tick,
            r.agent_inline as f64 + r.agent_heap,
            r.build_ns,
            common::checksum(e.scenario(), &r.checksum),
        );
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
    let (nodes, leaves, depth) = s.spec().shape();
    println!(
        "\n### {} — {nodes} nodes, {leaves} leaves, depth {depth}; {success} success / {failure} failure / {running} running per 100k ticks\n",
        s.name()
    );
    if s == Scenario::Soldier {
        let effects = &reference.0.soldier.effects;
        let mix: Vec<String> = soldier::Effect::NAMES
            .iter()
            .zip(effects)
            .map(|(name, n)| format!("{name} {n}"))
            .collect();
        println!("Actions completed per 100k ticks: {}.\n", mix.join(", "));
    }
    if s == Scenario::Villager {
        let v = &reference.0.villager;
        let mix: Vec<String> = villager::Effect::NAMES
            .iter()
            .zip(&v.effects)
            .map(|(name, n)| format!("{name} {n}"))
            .collect();
        println!(
            "Actions completed per 100k ticks: {}; {} random draws.\n",
            mix.join(", "),
            v.draws
        );
    }
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
