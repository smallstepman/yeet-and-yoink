use clap::{Parser, Subcommand};
use std::path::PathBuf;
use yeet_and_yoink::commands;
use yeet_and_yoink::commands::focus_or_cycle::FocusOrCycleArgs;
use yeet_and_yoink::commands::resize::ResizeMode;
use yeet_and_yoink::config;
use yeet_and_yoink::engine::topology::Direction;
use yeet_and_yoink::logging;

#[derive(Parser)]
#[command(
    name = "yeet-and-yoink",
    about = "Deep focus/move integration for niri"
)]
struct Cli {
    /// Write debug logs to a file.
    #[arg(long, global = true, value_name = "PATH")]
    log_file: Option<PathBuf>,

    /// Append to --log-file instead of truncating the file.
    #[arg(long, global = true, requires = "log_file")]
    log_append: bool,

    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Focus in a direction, navigating within apps before crossing window boundaries.
    Focus {
        #[arg(value_enum)]
        direction: Direction,
    },
    /// Move in a direction, tearing app buffers into new windows at boundaries.
    Move {
        #[arg(value_enum)]
        direction: Direction,
    },
    /// Resize in a direction, preferring in-app pane resize before compositor fallback.
    Resize {
        #[arg(value_enum)]
        direction: Direction,
        #[arg(value_enum, default_value_t = ResizeMode::Grow)]
        mode: ResizeMode,
    },
    /// Focus existing app instance, cycle through instances, or spawn if absent.
    FocusOrCycle {
        #[command(flatten)]
        args: FocusOrCycleArgs,
    },
}

fn main() {
    let cli = Cli::parse();
    logging::init(cli.log_file.as_deref(), cli.log_append);
    logging::debug(format!("argv={:?}", std::env::args().collect::<Vec<_>>()));

    let result = match config::prepare() {
        Ok(()) => match cli.command {
            Cmd::Focus { direction } => commands::focus::run(direction),
            Cmd::Move { direction } => commands::move_win::run(direction),
            Cmd::Resize { direction, mode } => commands::resize::run(direction, mode),
            Cmd::FocusOrCycle { args } => commands::focus_or_cycle::run(args),
        },
        Err(err) => Err(err),
    };

    if let Err(e) = result {
        logging::debug(format!("command failed: {e:#}"));
        eprintln!("yeet-and-yoink: {e:#}");
        std::process::exit(1);
    }

    logging::debug("command completed successfully");
}
