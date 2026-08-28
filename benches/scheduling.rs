#[path = "../tests/support/scheduling_fixture.rs"]
mod scheduling_fixture;
#[allow(dead_code)]
#[path = "../tests/support/scheduling_harness.rs"]
mod scheduling_harness;

use scheduling_fixture::{FixtureSize, SchedulingFixture};
use scheduling_harness::{SchedulingFreeTimeManager, SchedulingRepository};
use schronu::application::benchmarking::{
    flatten_tasks_diagnostics, get_schedule_diagnostics, pack_tasks_diagnostics,
};
use std::env;
use std::process::ExitCode;
use std::time::Instant;

const DAILY_FREE_MINUTES: i64 = 8 * 60;

#[derive(Clone, Copy)]
enum UseCase {
    All,
    Schedule,
    Pack,
    Flatten,
}

fn main() -> ExitCode {
    let arguments = env::args()
        .skip(1)
        .filter(|argument| argument != "--bench")
        .collect::<Vec<_>>();
    let configuration = match parse_configuration(&arguments) {
        Ok(configuration) => configuration,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::FAILURE;
        }
    };

    match run(configuration.0, configuration.1) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("scheduling benchmark failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn parse_configuration(arguments: &[String]) -> Result<(FixtureSize, UseCase), String> {
    let size = match arguments.first().map(String::as_str).unwrap_or("small") {
        "small" => Ok(FixtureSize::Small),
        "typical" => Ok(FixtureSize::Typical),
        "stress" => Ok(FixtureSize::Stress),
        other => Err(format!(
            "unknown fixture size {other:?}; expected small, typical, or stress"
        )),
    }?;
    let use_case = match arguments.get(1).map(String::as_str) {
        None if size == FixtureSize::Small => UseCase::All,
        None => {
            return Err(
                "typical and stress require an explicit use case: schedule, pack, or flatten"
                    .to_string(),
            )
        }
        Some("all") if size == FixtureSize::Small => UseCase::All,
        Some("schedule") => UseCase::Schedule,
        Some("pack") => UseCase::Pack,
        Some("flatten") => UseCase::Flatten,
        Some(other) => {
            return Err(format!(
                "unknown use case {other:?}; expected schedule, pack, or flatten"
            ))
        }
    };
    if arguments.len() > 2 {
        return Err("too many arguments; expected [size] [use-case]".to_string());
    }
    Ok((size, use_case))
}

fn run(size: FixtureSize, use_case: UseCase) -> Result<(), String> {
    println!("fixture={}", size_label(size));

    match use_case {
        UseCase::All => {
            run_schedule(size)?;
            run_pack(size)?;
            run_flatten(size)
        }
        UseCase::Schedule => run_schedule(size),
        UseCase::Pack => run_pack(size),
        UseCase::Flatten => run_flatten(size),
    }
}

fn run_schedule(size: FixtureSize) -> Result<(), String> {
    let fixture = SchedulingFixture::build(size).map_err(|error| error.to_string())?;
    let summary = fixture.summary().map_err(|error| error.to_string())?;
    println!(
        "fixture_seed={} fixture_digest={} projects={} tasks={} active_leaves={}",
        fixture.seed,
        fixture.digest().map_err(|error| error.to_string())?,
        summary.projects,
        summary.tasks,
        summary.active_leaves
    );
    let repository = SchedulingRepository::new(fixture.projects, fixture.now);
    let started = Instant::now();
    let (schedule, metrics) =
        get_schedule_diagnostics(&repository).map_err(|error| error.to_string())?;
    println!(
        "schedule elapsed_ms={:.3} output_segments={} metrics={metrics:?}",
        started.elapsed().as_secs_f64() * 1_000.0,
        schedule.len()
    );
    Ok(())
}

fn run_pack(size: FixtureSize) -> Result<(), String> {
    let fixture = SchedulingFixture::build(size).map_err(|error| error.to_string())?;
    let repository = SchedulingRepository::new(fixture.projects, fixture.now);
    let mut free_time_manager = SchedulingFreeTimeManager::new(DAILY_FREE_MINUTES);
    let started = Instant::now();
    let (result, metrics) = pack_tasks_diagnostics(&repository, &mut free_time_manager)
        .map_err(|error| error.to_string())?;
    println!(
        "pack elapsed_ms={:.3} packed={} skipped={} metrics={metrics:?}",
        started.elapsed().as_secs_f64() * 1_000.0,
        result.packed_tasks.len(),
        result.skipped_tasks.len()
    );
    Ok(())
}

fn run_flatten(size: FixtureSize) -> Result<(), String> {
    let fixture = SchedulingFixture::build(size).map_err(|error| error.to_string())?;
    let repository = SchedulingRepository::new(fixture.projects, fixture.now);
    let mut free_time_manager = SchedulingFreeTimeManager::new(DAILY_FREE_MINUTES);
    let started = Instant::now();
    let (result, metrics) = flatten_tasks_diagnostics(&repository, &mut free_time_manager)
        .map_err(|error| error.to_string())?;
    println!(
        "flatten elapsed_ms={:.3} flattened={} unresolved={} metrics={metrics:?}",
        started.elapsed().as_secs_f64() * 1_000.0,
        result.flattened_tasks.len(),
        result.unresolved_overloads.len()
    );

    Ok(())
}

fn size_label(size: FixtureSize) -> &'static str {
    match size {
        FixtureSize::Small => "small",
        FixtureSize::Typical => "typical",
        FixtureSize::Stress => "stress",
    }
}
