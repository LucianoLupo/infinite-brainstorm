// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use infinite_brainstorm_lib::{
    default_board_path, load_board_at, query_board, render_board_svg, validate_board_text,
    ExportOptions, ExportView, NodeFilter,
};

/// Infinite Brainstorm — agent-native infinite canvas.
///
/// With no subcommand, launches the desktop app (reads/writes `./board.json` in
/// the current working directory). The `validate`, `query`, and `export`
/// subcommands are headless helpers for agents to inspect or render a board
/// without opening the UI.
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
    /// Render a board to an image WITHOUT opening the GUI (read-only). Writes an
    /// SVG to `--out`; positions the camera with `--fit` (default), `--region`,
    /// or `--camera`, optionally restricting to `--nodes`/`--group`.
    Export {
        /// Path to the board file (defaults to ./board.json in the cwd).
        board: Option<PathBuf>,
        /// Output image path. Extension selects the format: `.svg` is supported;
        /// `.png` is a documented follow-up (headless PNG not yet implemented).
        #[arg(long)]
        out: PathBuf,
        /// Fit all (filtered) nodes with padding. This is the default when no
        /// view flag is given.
        #[arg(long, group = "view")]
        fit: bool,
        /// Frame an explicit world-space region "X,Y,W,H".
        #[arg(long, group = "view")]
        region: Option<String>,
        /// Use an explicit camera "X,Y,ZOOM".
        #[arg(long, group = "view")]
        camera: Option<String>,
        /// Restrict to a comma-separated list of node ids.
        #[arg(long, group = "subset")]
        nodes: Option<String>,
        /// Restrict to nodes in a single group.
        #[arg(long, group = "subset")]
        group: Option<String>,
        /// Output width in pixels.
        #[arg(long, default_value_t = 1600)]
        width: u32,
        /// Output height in pixels.
        #[arg(long, default_value_t = 1000)]
        height: u32,
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

/// Parse a comma-separated list of `n` finite floats, erroring on the wrong
/// count or any non-numeric component. Used for `--region`/`--camera`.
fn parse_floats(label: &str, raw: &str, n: usize) -> Result<Vec<f64>, String> {
    let parts: Vec<&str> = raw.split(',').collect();
    if parts.len() != n {
        return Err(format!(
            "--{label} expects {n} comma-separated numbers, got {}: '{raw}'",
            parts.len()
        ));
    }
    parts
        .iter()
        .map(|p| {
            p.trim()
                .parse::<f64>()
                .map_err(|_| format!("--{label}: '{}' is not a number", p.trim()))
        })
        .collect()
}

/// Resolve the mutually-exclusive view flags into an [`ExportView`]. Defaults to
/// `Fit` when none is given (clap's `group` already enforces at most one).
fn resolve_view(
    fit: bool,
    region: Option<String>,
    camera: Option<String>,
) -> Result<ExportView, String> {
    let _ = fit; // `--fit` is the default; the flag just makes intent explicit.
    if let Some(r) = region {
        let v = parse_floats("region", &r, 4)?;
        Ok(ExportView::Region {
            x: v[0],
            y: v[1],
            w: v[2],
            h: v[3],
        })
    } else if let Some(c) = camera {
        let v = parse_floats("camera", &c, 3)?;
        Ok(ExportView::Camera {
            x: v[0],
            y: v[1],
            zoom: v[2],
        })
    } else {
        Ok(ExportView::Fit)
    }
}

/// Run the `export` subcommand. Loads the board read-only, resolves the view +
/// node filter, and writes an SVG to `--out`. PNG (and any other extension) is a
/// non-zero error with a documented message. Never writes the board file.
#[allow(clippy::too_many_arguments)]
fn run_export(
    board: Option<PathBuf>,
    out: PathBuf,
    fit: bool,
    region: Option<String>,
    camera: Option<String>,
    nodes: Option<String>,
    group: Option<String>,
    width: u32,
    height: u32,
) -> ExitCode {
    let path = match resolve_path(board) {
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

    let view = match resolve_view(fit, region, camera) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };

    let filter = match (nodes, group) {
        (Some(ids), _) => NodeFilter::Ids(
            ids.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
        ),
        (None, Some(g)) => NodeFilter::Group(g),
        (None, None) => NodeFilter::All,
    };

    // Branch on the output extension.
    let ext = out
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    match ext.as_deref() {
        Some("svg") => {
            let opts = ExportOptions {
                width,
                height,
                view,
            };
            let svg = match render_board_svg(&board, &filter, &opts) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("error: {e}");
                    return ExitCode::FAILURE;
                }
            };
            if let Err(e) = std::fs::write(&out, svg) {
                eprintln!("error: cannot write {}: {e}", out.display());
                return ExitCode::FAILURE;
            }
            println!("{}", out.display());
            ExitCode::SUCCESS
        }
        Some("png") => {
            eprintln!(
                "error: headless PNG export is not yet supported; export to .svg and \
rasterize externally (e.g. `rsvg-convert`/`resvg`). Tracking: PNG follow-up."
            );
            ExitCode::FAILURE
        }
        _ => {
            eprintln!(
                "error: unsupported output extension for {}; use .svg",
                out.display()
            );
            ExitCode::FAILURE
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    match cli.command {
        Some(Command::Validate { path }) => run_validate(path),
        Some(Command::Query { expr, path }) => run_query(expr, path),
        Some(Command::Export {
            board,
            out,
            fit,
            region,
            camera,
            nodes,
            group,
            width,
            height,
        }) => run_export(board, out, fit, region, camera, nodes, group, width, height),
        None => {
            // No subcommand: launch the desktop GUI. `run()` blocks until the
            // window closes and exits the process on a fatal Tauri error, so it
            // never returns normally — but return SUCCESS for completeness.
            infinite_brainstorm_lib::run();
            ExitCode::SUCCESS
        }
    }
}
