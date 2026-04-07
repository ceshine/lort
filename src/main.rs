use std::io::{self, IsTerminal as _, Read as _};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::Parser;
use owo_colors::OwoColorize;
use similar::{ChangeTag, TextDiff};
use tracing::{error, info, warn};
use walkdir::WalkDir;

use lort::error::LortError;
use lort::parser::parse_file;
use lort::sorter::sort_and_reconstruct;

/// lort — a Python import sorter with length-first segment ordering.
///
/// Sorts imports within each contiguous block using length-based
/// segment comparison, placing shorter module names before longer ones.
/// Designed to run alongside `ruff format` and `ruff check`.
#[derive(Parser, Debug)]
#[command(name = "lort", version, about)]
struct Cli {
    /// Files or directories to process. Defaults to current directory.
    #[arg(default_value = ".")]
    paths: Vec<PathBuf>,

    /// Check mode: report violations without modifying files.
    /// Exit code 1 if any file is unsorted.
    #[arg(short = 'c', long)]
    check: bool,

    /// Show a unified diff of what would change (implies --check).
    #[arg(short = 'd', long)]
    diff: bool,

    /// Suppress all output except errors.
    #[arg(short = 'q', long)]
    quiet: bool,

    /// Show each file processed and whether it was modified/already sorted.
    #[arg(short = 'v', long)]
    verbose: bool,

    /// Glob patterns to exclude (e.g. --exclude "migrations/*").
    /// Can be specified multiple times.
    #[arg(short = 'e', long)]
    exclude: Vec<String>,

    /// Read from stdin, write sorted result to stdout.
    #[arg(long)]
    stdin: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    // Initialize tracing subscriber.
    // Default level is WARN; --verbose promotes to INFO; --quiet suppresses to ERROR.
    // Users can override via the RUST_LOG environment variable.
    let default_level = if cli.verbose {
        "info"
    } else if cli.quiet {
        "error"
    } else {
        "warn"
    };

    // NOTE for Rust newcomers: `tracing-subscriber` decouples log output
    // format from the log call sites. EnvFilter lets users override the
    // level at runtime via RUST_LOG=debug, for example.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(default_level)),
        )
        .without_time()
        .with_target(false)
        .init();

    match run(&cli) {
        Ok(has_unsorted) => {
            if has_unsorted && (cli.check || cli.diff) {
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(e) => {
            // Check if the underlying error is a LortError (exit code 2).
            if let Some(lort_err) = e.downcast_ref::<LortError>() {
                error!("{lort_err}");
            } else {
                error!("{e}");
            }
            ExitCode::from(2)
        }
    }
}

/// Main logic: process files and return whether any were unsorted.
fn run(cli: &Cli) -> Result<bool> {
    // --diff implies --check.
    let check_mode = cli.check || cli.diff;

    if cli.stdin {
        return process_stdin(check_mode, cli.diff);
    }

    let files = collect_python_files(&cli.paths, &cli.exclude)?;

    if files.is_empty() && cli.verbose {
        warn!("No Python files found.");
    }

    // Enable ANSI colors when stdout is a terminal.
    let use_color = io::stdout().is_terminal();

    let mut any_unsorted = false;

    for file_path in &files {
        let changed = process_file(
            file_path,
            check_mode,
            cli.diff,
            cli.quiet,
            cli.verbose,
            use_color,
        )?;
        if changed {
            any_unsorted = true;
        }
    }

    Ok(any_unsorted)
}

/// Process a single Python file.
///
/// Returns `true` if the file's imports were not already sorted.
fn process_file(
    file_path: &Path,
    check_mode: bool,
    show_diff: bool,
    quiet: bool,
    verbose: bool,
    use_color: bool,
) -> Result<bool> {
    let source = std::fs::read_to_string(file_path).map_err(|e| LortError::Io {
        path: file_path.to_path_buf(),
        source: e,
    })?;

    let segments = parse_file(&source, file_path)?;
    let sorted = sort_and_reconstruct(segments, file_path)?;

    if source == sorted {
        if verbose {
            info!(file = %file_path.display(), "already sorted");
        }
        return Ok(false);
    }

    // File needs changes.
    if check_mode {
        if show_diff {
            print_diff(file_path, &source, &sorted, use_color);
        } else if !quiet {
            warn!(file = %file_path.display(), "would be resorted");
        }
    } else {
        std::fs::write(file_path, &sorted).map_err(|e| LortError::Io {
            path: file_path.to_path_buf(),
            source: e,
        })?;
        if !quiet {
            info!(file = %file_path.display(), "sorted");
        }
    }

    Ok(true)
}

/// Read from stdin, sort, write to stdout.
fn process_stdin(check_mode: bool, show_diff: bool) -> Result<bool> {
    let mut source = String::new();
    io::stdin()
        .read_to_string(&mut source)
        .context("failed to read from stdin")?;

    let stdin_path = PathBuf::from("<stdin>");
    let segments = parse_file(&source, &stdin_path)?;
    let sorted = sort_and_reconstruct(segments, &stdin_path)?;

    if source == sorted {
        print!("{source}");
        return Ok(false);
    }

    if check_mode && show_diff {
        // stdin mode: color only if stdout is a terminal.
        let use_color = io::stdout().is_terminal();
        print_diff(&stdin_path, &source, &sorted, use_color);
    } else {
        print!("{sorted}");
    }

    Ok(source != sorted)
}

/// Print a unified diff between original and sorted content.
///
/// When `use_color` is true, applies ANSI colors:
/// - Red for removed lines (`-`)
/// - Green for added lines (`+`)
/// - Cyan for hunk headers (`@@`)
/// - Bold for file headers (`---`/`+++`)
fn print_diff(path: &Path, original: &str, sorted: &str, use_color: bool) {
    let diff = TextDiff::from_lines(original, sorted);
    let display_path = path.display();

    if use_color {
        println!("{}", format_args!("--- {display_path}").bold());
        println!("{}", format_args!("+++ {display_path}").bold());
    } else {
        println!("--- {display_path}");
        println!("+++ {display_path}");
    }

    // Iterate over hunks and their individual change lines for
    // per-line coloring. The `unified_diff` formatter only gives
    // us pre-rendered strings — iterating changes directly gives
    // control over each line's color.
    for hunk in diff.unified_diff().context_radius(3).iter_hunks() {
        let header = hunk.header().to_string();
        if use_color {
            println!("{}", header.cyan());
        } else {
            print!("{header}");
        }

        for change in hunk.iter_changes() {
            let sign = match change.tag() {
                ChangeTag::Delete => "-",
                ChangeTag::Insert => "+",
                ChangeTag::Equal => " ",
            };
            let line = format!("{sign}{change}");

            if use_color {
                match change.tag() {
                    ChangeTag::Delete => print!("{}", line.red()),
                    ChangeTag::Insert => print!("{}", line.green()),
                    ChangeTag::Equal => print!("{line}"),
                }
            } else {
                print!("{line}");
            }

            // `similar` change values don't always end with a newline
            // (e.g. the last line of a file without a trailing newline).
            if change.missing_newline() {
                println!();
            }
        }
    }
}

/// Recursively collect all `.py` files from the given paths,
/// respecting exclude patterns.
fn collect_python_files(paths: &[PathBuf], exclude_patterns: &[String]) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    for path in paths {
        if path.is_file() {
            if path.extension().is_some_and(|ext| ext == "py") {
                files.push(path.clone());
            }
        } else if path.is_dir() {
            for entry in WalkDir::new(path)
                .into_iter()
                .filter_entry(|e| !is_hidden(e))
            {
                let entry = entry
                    .with_context(|| format!("failed to walk directory: {}", path.display()))?;
                let entry_path = entry.path();

                if entry_path.is_file()
                    && entry_path.extension().is_some_and(|ext| ext == "py")
                    && !is_excluded(entry_path, exclude_patterns)
                {
                    files.push(entry_path.to_path_buf());
                }
            }
        }
    }

    // Sort for deterministic output order.
    files.sort();
    Ok(files)
}

/// Check if a directory entry is hidden (starts with `.`).
fn is_hidden(entry: &walkdir::DirEntry) -> bool {
    entry
        .file_name()
        .to_str()
        .is_some_and(|s| s.starts_with('.') && s != ".")
}

/// Check if a path matches any of the exclude glob patterns.
///
/// Uses simple substring matching for now. A full glob library
/// could be added later if needed.
fn is_excluded(path: &Path, patterns: &[String]) -> bool {
    let path_str = path.to_string_lossy();
    patterns.iter().any(|pat| {
        // Simple wildcard matching: treat `*` as "match anything".
        if let Some(suffix) = pat.strip_prefix('*') {
            path_str.ends_with(suffix)
        } else if let Some(prefix) = pat.strip_suffix('*') {
            path_str.contains(prefix)
        } else {
            path_str.contains(pat.as_str())
        }
    })
}
