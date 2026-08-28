#[path = "../tests/support/scheduling_benchmark_limits.rs"]
mod scheduling_benchmark_limits;
#[path = "../tests/support/scheduling_fixture.rs"]
mod scheduling_fixture;
#[allow(dead_code)]
#[path = "../tests/support/scheduling_harness.rs"]
mod scheduling_harness;

use scheduling_benchmark_limits::FLATTEN_BENCHMARK_CAPACITY_MINUTES;
use scheduling_fixture::{FixtureSize, SchedulingFixture};
use scheduling_harness::{SchedulingFreeTimeManager, SchedulingRepository};
use schronu::application::benchmarking::{
    flatten_tasks_diagnostics, get_schedule_diagnostics, pack_tasks_diagnostics,
};
use std::env;
use std::process::ExitCode;
use std::time::{Duration, Instant};

const DAILY_FREE_MINUTES: i64 = 8 * 60;
const SAMPLE_COUNT: usize = 3;
const TYPICAL_LIMIT: Duration = Duration::from_millis(500);
const STRESS_LIMIT: Duration = Duration::from_secs(5);

#[derive(Clone, Copy)]
enum UseCase {
    All,
    Schedule,
    Pack,
    Flatten,
}

#[derive(Clone, Copy)]
struct Configuration {
    size: FixtureSize,
    use_case: UseCase,
    check_limit: bool,
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

    match run(configuration) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("scheduling benchmark failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn parse_configuration(arguments: &[String]) -> Result<Configuration, String> {
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
    let check_limit = match arguments.get(2).map(String::as_str) {
        None => false,
        Some("check") => true,
        Some(other) => return Err(format!("unknown mode {other:?}; expected check")),
    };
    if arguments.len() > 3 {
        return Err("too many arguments; expected [size] [use-case] [check]".to_string());
    }
    Ok(Configuration {
        size,
        use_case,
        check_limit,
    })
}

fn run(configuration: Configuration) -> Result<(), String> {
    println!("fixture={}", size_label(configuration.size));

    match configuration.use_case {
        UseCase::All => {
            run_schedule(configuration)?;
            run_pack(configuration)?;
            run_flatten(configuration)
        }
        UseCase::Schedule => run_schedule(configuration),
        UseCase::Pack => run_pack(configuration),
        UseCase::Flatten => run_flatten(configuration),
    }
}

fn run_schedule(configuration: Configuration) -> Result<(), String> {
    let mut samples = Vec::with_capacity(SAMPLE_COUNT);
    let mut last_output = None;
    for sample_index in 0..SAMPLE_COUNT {
        let fixture = SchedulingFixture::build(configuration.size).map_err(|e| e.to_string())?;
        if sample_index == 0 {
            let summary = fixture.summary().map_err(|error| error.to_string())?;
            println!(
                "fixture_seed={} fixture_digest={} projects={} tasks={} active_leaves={}",
                fixture.seed,
                fixture.digest().map_err(|error| error.to_string())?,
                summary.projects,
                summary.tasks,
                summary.active_leaves
            );
        }
        let repository = SchedulingRepository::new(fixture.projects, fixture.now);
        let started = Instant::now();
        let output = get_schedule_diagnostics(&repository).map_err(|error| error.to_string())?;
        samples.push(started.elapsed());
        last_output = Some(output);
    }
    let elapsed = median(&mut samples);
    check_limit(configuration, elapsed)?;
    let (schedule, metrics) = last_output.expect("sample count is positive");
    println!(
        "schedule median_ms={:.3} samples={} output_segments={} metrics={metrics:?}",
        elapsed.as_secs_f64() * 1_000.0,
        SAMPLE_COUNT,
        schedule.len()
    );
    Ok(())
}

fn run_pack(configuration: Configuration) -> Result<(), String> {
    let mut samples = Vec::with_capacity(SAMPLE_COUNT);
    let mut last_output = None;
    for _ in 0..SAMPLE_COUNT {
        let fixture = SchedulingFixture::build(configuration.size).map_err(|e| e.to_string())?;
        let repository = SchedulingRepository::new(fixture.projects, fixture.now);
        let mut free_time_manager = SchedulingFreeTimeManager::new(DAILY_FREE_MINUTES);
        let started = Instant::now();
        let output = pack_tasks_diagnostics(&repository, &mut free_time_manager)
            .map_err(|error| error.to_string())?;
        samples.push(started.elapsed());
        last_output = Some(output);
    }
    let elapsed = median(&mut samples);
    check_limit(configuration, elapsed)?;
    let (result, metrics) = last_output.expect("sample count is positive");
    println!(
        "pack median_ms={:.3} samples={} packed={} skipped={} metrics={metrics:?}",
        elapsed.as_secs_f64() * 1_000.0,
        SAMPLE_COUNT,
        result.packed_tasks.len(),
        result.skipped_tasks.len()
    );
    Ok(())
}

fn run_flatten(configuration: Configuration) -> Result<(), String> {
    let mut samples = Vec::with_capacity(SAMPLE_COUNT);
    let mut last_output = None;
    for _ in 0..SAMPLE_COUNT {
        let fixture = SchedulingFixture::build(configuration.size).map_err(|e| e.to_string())?;
        let repository = SchedulingRepository::new(fixture.projects, fixture.now);
        let mut free_time_manager =
            SchedulingFreeTimeManager::new(FLATTEN_BENCHMARK_CAPACITY_MINUTES);
        let started = Instant::now();
        let output = flatten_tasks_diagnostics(&repository, &mut free_time_manager)
            .map_err(|error| error.to_string())?;
        samples.push(started.elapsed());
        last_output = Some(output);
    }
    let elapsed = median(&mut samples);
    check_limit(configuration, elapsed)?;
    let (result, metrics) = last_output.expect("sample count is positive");
    println!(
        "flatten median_ms={:.3} samples={} flattened={} unresolved={} metrics={metrics:?}",
        elapsed.as_secs_f64() * 1_000.0,
        SAMPLE_COUNT,
        result.flattened_tasks.len(),
        result.unresolved_overloads.len()
    );

    Ok(())
}

fn median(samples: &mut [Duration]) -> Duration {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

fn check_limit(configuration: Configuration, elapsed: Duration) -> Result<(), String> {
    if !configuration.check_limit {
        return Ok(());
    }
    let limit = match configuration.size {
        FixtureSize::Small => return Ok(()),
        FixtureSize::Typical => TYPICAL_LIMIT,
        FixtureSize::Stress => STRESS_LIMIT,
    };
    if elapsed <= limit {
        Ok(())
    } else {
        Err(format!(
            "{} {} median {:.3}ms exceeded {:.3}ms",
            size_label(configuration.size),
            use_case_label(configuration.use_case),
            elapsed.as_secs_f64() * 1_000.0,
            limit.as_secs_f64() * 1_000.0
        ))
    }
}

fn use_case_label(use_case: UseCase) -> &'static str {
    match use_case {
        UseCase::All => "all",
        UseCase::Schedule => "schedule",
        UseCase::Pack => "pack",
        UseCase::Flatten => "flatten",
    }
}

fn size_label(size: FixtureSize) -> &'static str {
    match size {
        FixtureSize::Small => "small",
        FixtureSize::Typical => "typical",
        FixtureSize::Stress => "stress",
    }
}
