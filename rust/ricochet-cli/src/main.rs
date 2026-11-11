//! Port of src/Ricochet.Cli/Program.cs (ADR 0091): runs a Workspace's Requests and checks for CI (`run`), and exports,
//! documents, zips, clones and moves Environments (`export`, `docs`, `zip`, `clone`, `env`). The exit codes match the
//! C# CLI: 0 passed, 1 a Request or check failed (or an import was refused), 2 the command could not run.

mod commands;
mod run;

use ricochet_core::collection_runner::EXIT_CANNOT_RUN;

const USAGE: &str = "Ricochet: run a workspace's requests and check their assertions.

Usage:
  ricochet run [folder] [options]   (see ricochet run --help)
  ricochet export <collection> --postman <file> | --openapi <file>   (see ricochet export --help)
  ricochet env export <environment> <file.json> | env import <file.json>   (see ricochet env --help)
  ricochet docs <collection> <file.html> [--force]   (see ricochet docs --help)
  ricochet zip export <workspace> <file.zip> | zip import <file.zip> <new-folder>   (see ricochet zip --help)
  ricochet clone <git-url> <target-folder>   (see ricochet clone --help)

Exit codes:
  0  Everything passed, or help was shown.
  1  A request or assertion failed, in any data row.
  2  The run could not happen, for example because a workspace file or the data file is unreadable.";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(dispatch(&args));
}

fn dispatch(args: &[String]) -> i32 {
    let rest = args.get(1..).unwrap_or_default();
    match args.first().map(String::as_str) {
        None => {
            println!("{USAGE}");
            EXIT_CANNOT_RUN
        }
        Some("--help" | "-h" | "help") => {
            println!("{USAGE}");
            0
        }
        Some("run") => run::run_command(rest),
        Some("export") => commands::export_command(rest),
        Some("env") => commands::env_command(rest),
        Some("docs") => commands::docs_command(rest),
        Some("zip") => commands::zip_command(rest),
        Some("clone") => commands::clone_command(rest),
        Some(other) => commands::cannot_run(format!("Unknown command \"{other}\". Try: ricochet --help")),
    }
}
