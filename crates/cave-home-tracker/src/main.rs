// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors

//! `cave-home-tracker` command-line entry point.
//!
//! ```text
//! cave-home-tracker --config tracker.yaml poll        # clone/update upstreams
//! cave-home-tracker --config tracker.yaml measure     # snapshot LOC/tests/stubs
//! cave-home-tracker --config tracker.yaml diff        # day-over-day deltas
//! cave-home-tracker --config tracker.yaml report      # daily markdown report
//! cave-home-tracker --config tracker.yaml dashboard   # Prometheus :9102/metrics
//! ```

use std::path::PathBuf;
use std::process::ExitCode;

use chrono::Local;
use clap::{Parser, Subcommand};

use cave_home_tracker::config::TrackerConfig;
use cave_home_tracker::git::{ShellGit, poll_all, poll_one};
use cave_home_tracker::measure::{CargoTestRunner, Measurer, NoopTestRunner, TestRunner};
use cave_home_tracker::snapshot::SnapshotStore;
use cave_home_tracker::{Result, dashboard, diff, report};

/// Persistent upstream delta tracker for cave-home (and cave-runtime).
#[derive(Debug, Parser)]
#[command(name = "cave-home-tracker", version, about)]
struct Cli {
    /// Path to the tracker config (YAML).
    #[arg(
        long,
        short,
        default_value = "tracker.yaml",
        env = "CAVE_TRACKER_CONFIG"
    )]
    config: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Shallow-clone (or update) tracked upstreams.
    Poll {
        /// Only poll these upstreams (by name); default polls all.
        #[arg(long = "upstream")]
        upstreams: Vec<String>,
    },
    /// Measure LOC / tests / stubs and write a dated snapshot.
    Measure {
        /// Skip running tests (LOC + stubs only; much faster).
        #[arg(long)]
        no_tests: bool,
    },
    /// Print the day-over-day diff against the previous snapshot.
    Diff {
        /// Compare against this exact date (YYYY-MM-DD) instead of the previous.
        #[arg(long)]
        against: Option<String>,
    },
    /// Render the daily markdown progress report.
    Report {
        /// Write to this path instead of `docs/audit/daily-progress-<date>.md`.
        #[arg(long, short)]
        output: Option<PathBuf>,
        /// Print to stdout instead of writing a file.
        #[arg(long)]
        stdout: bool,
    },
    /// Serve Prometheus metrics for the latest snapshot.
    Dashboard {
        /// Address to bind. Defaults to :9102 (9100 is node_exporter's port).
        #[arg(long, default_value = "0.0.0.0:9102")]
        addr: String,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("cave-home-tracker: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: &Cli) -> Result<()> {
    let cfg = TrackerConfig::from_path(&cli.config)?;
    match &cli.command {
        Command::Poll { upstreams } => {
            cmd_poll(&cfg, upstreams);
            Ok(())
        }
        Command::Measure { no_tests } => cmd_measure(&cfg, *no_tests),
        Command::Diff { against } => cmd_diff(&cfg, against.as_deref()),
        Command::Report { output, stdout } => cmd_report(&cfg, output.clone(), *stdout),
        Command::Dashboard { addr } => cmd_dashboard(&cfg, addr),
    }
}

fn cmd_poll(cfg: &TrackerConfig, only: &[String]) {
    let git = ShellGit::new();
    // Honour `--upstream`: poll only the requested upstreams, not all of them.
    let results = if only.is_empty() {
        poll_all(cfg, &git)
    } else {
        only.iter()
            .map(|name| (name.clone(), poll_one(cfg, &git, name)))
            .collect()
    };
    let mut failures = 0;
    for (name, res) in results {
        match res {
            Ok(p) => {
                let tag = p.latest_tag.as_deref().unwrap_or("(no tag)");
                let short = &p.head_commit[..p.head_commit.len().min(12)];
                println!("poll  {name:<22} {tag:<16} {short}");
            }
            Err(e) => {
                eprintln!("poll  {name:<22} FAILED: {e}");
                failures += 1;
            }
        }
    }
    if failures > 0 {
        eprintln!("{failures} upstream(s) failed to poll");
    }
}

fn cmd_measure(cfg: &TrackerConfig, no_tests: bool) -> Result<()> {
    let now = Local::now();
    let date = now.format("%Y-%m-%d").to_string();
    let generated_at = now.to_rfc3339();

    let cargo = CargoTestRunner::default();
    let noop = NoopTestRunner;
    let runner: &dyn TestRunner = if no_tests { &noop } else { &cargo };

    let measurer = Measurer::new(cfg, runner);
    let snap = measurer.measure_all(&date, &generated_at)?;

    let store = SnapshotStore::open(cfg.snapshots_dir())?;
    let path = store.save(&snap)?;

    println!(
        "measured {} subsystems → {}",
        snap.subsystems.len(),
        path.display()
    );
    println!(
        "overall: {:.1}%   k3s: {:.1}%   smart-home: {:.1}%",
        snap.overall_real_pct(),
        snap.group_real_pct("k3s"),
        snap.complement_real_pct("k3s")
    );
    Ok(())
}

fn cmd_diff(cfg: &TrackerConfig, against: Option<&str>) -> Result<()> {
    let store = SnapshotStore::open(cfg.snapshots_dir())?;
    let cur = store.latest()?.ok_or_else(|| {
        cave_home_tracker::TrackerError::NotFound("no snapshot; run `measure` first".into())
    })?;
    let prev = match against {
        Some(date) => store.load(date)?,
        None => store.previous(&cur.date)?,
    };
    let dd = diff::diff(prev.as_ref(), &cur);
    println!(
        "diff {} → {}",
        if dd.from_date.is_empty() {
            "(none)"
        } else {
            &dd.from_date
        },
        dd.to_date
    );
    println!("overall Δ {:+.1}%", dd.d_overall_real_pct);
    for d in &dd.subsystems {
        let tag = if d.is_new {
            "NEW".to_owned()
        } else {
            format!("{:+.1}%", d.d_real_pct)
        };
        println!(
            "  {:<22} real {:>7}  port {:+}  tests {:+}  stubs {:+}",
            d.name, tag, d.d_port_loc, d.d_tests_passed, d.d_stubs
        );
    }
    Ok(())
}

fn cmd_report(cfg: &TrackerConfig, output: Option<PathBuf>, to_stdout: bool) -> Result<()> {
    let store = SnapshotStore::open(cfg.snapshots_dir())?;
    let history = store.load_all()?;
    let cur = store.latest()?.ok_or_else(|| {
        cave_home_tracker::TrackerError::NotFound("no snapshot; run `measure` first".into())
    })?;
    let prev = store.previous(&cur.date)?;
    let dd = diff::diff(prev.as_ref(), &cur);
    let md = report::render_markdown(&cur, &dd, &history);

    if to_stdout {
        print!("{md}");
        return Ok(());
    }

    let path = output.unwrap_or_else(|| {
        cfg.root_path()
            .join("docs/audit")
            .join(format!("daily-progress-{}.md", cur.date))
    });
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| cave_home_tracker::TrackerError::io(parent, e))?;
    }
    std::fs::write(&path, md).map_err(|e| cave_home_tracker::TrackerError::io(&path, e))?;
    println!("report → {}", path.display());
    Ok(())
}

fn cmd_dashboard(cfg: &TrackerConfig, addr: &str) -> Result<()> {
    let store = SnapshotStore::open(cfg.snapshots_dir())?;
    println!("serving metrics on http://{addr}/metrics (Ctrl-C to stop)");
    dashboard::serve(addr, move || store.latest().ok().flatten())
}
