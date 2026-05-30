// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use infinite_brainstorm_lib::{
    default_board_path, load_board_at, query_board, validate_board_text,
};

/// Infinite Brainstorm — agent-native infinite canvas.
///
/// With no subcommand, launches the desktop app (reads/writes `./board.json` in
/// the current working directory). The `validate` and `query` subcommands are
/// headless helpers for agents to inspect a board without opening the UI.
#[derive(Parser)]
#[command(name = "infinite-brainstorm", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Validate a board.json, reporting duplicate/dangling/invalid data.
    /// Exits non-zero if any structural error is found.
    Validate {
        /// Path to the board file (defaults to ./board.json in the cwd).
        path: Option<PathBuf>,
    },
    /// Run a read-only query against a board and print the result.
    /// Examples: `count`, `nodes`, `edges`, `node:<id>`, `type:idea`, `tag:urgent`.
    Query {
        /// The query expression.
        expr: String,
        /// Path to the board file (defaults to ./board.json in the cwd).
        #[arg(long)]
        path: Option<PathBuf>,
    },
}

fn resolve_path(explicit: Option<PathBuf>) -> Result<PathBuf, String> {
    match explicit {
        Some(p) => Ok(p),
        None => default_board_path(),
    }
}

/// Run the `validate` subcommand. Prints every problem to stderr and returns a
/// non-zero exit code when any structural error exists. Unknown top-level keys
/// are warnings only (forward-compat) and never fail the command.
fn run_validate(path: Option<PathBuf>) -> ExitCode {
    let path = match resolve_path(path) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };

    if !path.exists() {
        eprintln!("error: board file not found: {}", path.display());
        return ExitCode::FAILURE;
    }

    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: cannot read {}: {e}", path.display());
            return ExitCode::FAILURE;
        }
    };

    let report = match validate_board_text(&raw) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {}: {e}", path.display());
            return ExitCode::FAILURE;
        }
    };

    // Forward-compat warnings (unknown keys + a future schema version) never fail
    // the command — print them and keep going.
    for key in &report.unknown_keys {
        eprintln!("warning: unknown top-level key '{key}' (ignored)");
    }
    for warn in report.warnings() {
        eprintln!("warning: {warn}");
    }
    let warning_count = report.unknown_keys.len() + report.warnings().count();

    let fatal: Vec<_> = report.fatal_errors().collect();
    if fatal.is_empty() {
        println!("{}: ok ({warning_count} warning(s))", path.display());
        ExitCode::SUCCESS
    } else {
        for err in &fatal {
            eprintln!("error: {err}");
        }
        eprintln!("{}: {} validation error(s)", path.display(), fatal.len());
        ExitCode::FAILURE
    }
}

/// Run the `query` subcommand. Prints the query result to stdout, or the error
/// (with a non-zero exit) if the board can't be loaded or the expression is
/// unsupported.
fn run_query(expr: String, path: Option<PathBuf>) -> ExitCode {
    let path = match resolve_path(path) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };

    let board = match load_board_at(&path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("error: {}: {e}", path.display());
            return ExitCode::FAILURE;
        }
    };

    match query_board(&board, &expr) {
        Ok(out) => {
            println!("{out}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    match cli.command {
        Some(Command::Validate { path }) => run_validate(path),
        Some(Command::Query { expr, path }) => run_query(expr, path),
        None => {
            // No subcommand: launch the desktop GUI. `run()` blocks until the
            // window closes and exits the process on a fatal Tauri error, so it
            // never returns normally — but return SUCCESS for completeness.
            infinite_brainstorm_lib::run();
            ExitCode::SUCCESS
        }
    }
}
