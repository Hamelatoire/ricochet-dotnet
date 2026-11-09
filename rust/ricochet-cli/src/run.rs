//! The `run` command, ported from `RunAsync`, `TryParse`, `PrintResult` and `ReadDataRows` in src/Ricochet.Cli/Program.cs.
//! It only reads the Workspace. Output is masked by the runner (ADR 0076), and data-file values are never printed.

use std::fs;
use std::io::{self, IsTerminal};
use std::path::Path;
use std::time::Duration;

use chrono::{DateTime, Local};
use ricochet_core::collection_runner::{
    self, CollectionRunner, EXIT_CANNOT_RUN, EXIT_PASSED, EXIT_TESTS_FAILED, RequestRunResult, RunOptions, RunReport,
};
use ricochet_core::data_file::{self, Row};
use ricochet_core::data_rows::{self, DataRowReport};
use ricochet_core::dotenv;
use ricochet_core::folder_tags;
use ricochet_core::models::KeyValue;
use ricochet_core::send::{RequestSender, SenderSettings};
use ricochet_core::store::{MARKER_FILE_NAME, WorkspaceFolder};
use ricochet_core::xunit;

/// The prefix of environment variables that become Variables for the run (ADR 0021).
const VARIABLE_PREFIX: &str = "RICOCHET_VAR_";
const COLLECTIONS_DIR: &str = "collections";

pub const RUN_USAGE: &str = "Ricochet: run a workspace's requests and check their assertions.

Usage:
  ricochet run [folder] [options]

Arguments:
  folder                 The workspace folder. Defaults to the current folder.

Options:
  --collection <name>    Run only this collection.
  --folder <name>        Run only requests inside folders with this name.
  --tag <name>           Run only requests with this tag, given by their folders. Repeatable; any match counts.
  --exclude-tag <name>   Skip requests with this tag. Repeatable; wins over --tag.
  --env <name>           Use this environment.
  --var <name=value>     Set a variable, overriding every scope. Repeatable.
  --data <file>          Run the collection once per row of a .csv or .json data file. Each row's columns
                         are variables for that run, and each run's results are printed under its row number.
  --xunit <file>         Also write an xUnit XML report.
  --timeout <seconds>    Time limit for each request. Default: 120.
  --bail                 Stop at the first failure.
  --cookies              Not ported yet. The run refuses it rather than keep no cookies silently.
  --insecure             Skip TLS certificate checks. Only for servers you trust, such as a local test server.
  --help                 Show this help.

Variables can also come from a .env file in the workspace folder, or from environment
variables named RICOCHET_VAR_<name>. Environment variables win over .env, and --var wins over both.
Prefer those for secrets in CI, because --var values can show up in process lists.
Values set either way are masked in the output.

Exit codes:
  0  Everything passed.
  1  A request or assertion failed, in any data row.
  2  The run could not happen, for example because a workspace file or the data file is unreadable.";

/// The options of one `run` command, as the command line gave them.
#[derive(Debug, Clone, PartialEq)]
pub struct RunArgs {
    pub folder: String,
    pub collection: Option<String>,
    pub folder_name: Option<String>,
    pub tags: Vec<String>,
    pub exclude_tags: Vec<String>,
    pub environment: Option<String>,
    pub xunit: Option<String>,
    pub data_file: Option<String>,
    pub timeout_seconds: i32,
    pub bail: bool,
    pub insecure: bool,
    pub cookies: bool,
    pub variables: Vec<KeyValue>,
}

impl Default for RunArgs {
    fn default() -> Self {
        Self {
            folder: ".".to_string(),
            collection: None,
            folder_name: None,
            tags: Vec::new(),
            exclude_tags: Vec::new(),
            environment: None,
            xunit: None,
            data_file: None,
            timeout_seconds: 120,
            bail: false,
            insecure: false,
            cookies: false,
            variables: Vec::new(),
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum Parsed {
    Run(RunArgs),
    /// `--help` was given. It is not a failure.
    Help,
    /// The command line can't be used. The message says why and how to get help.
    Error(String),
}

/// Parses the arguments after `run`. Mirrors `TryParse` in the C# CLI.
pub fn parse_args(args: &[String]) -> Parsed {
    let mut options = RunArgs::default();
    let mut positional = 0;
    let mut index = 0;

    while index < args.len() {
        let arg = args[index].as_str();
        match arg {
            "--help" | "-h" => return Parsed::Help,
            "--bail" => options.bail = true,
            "--insecure" => options.insecure = true,
            "--cookies" => options.cookies = true,
            "--collection" | "--folder" | "--env" | "--xunit" | "--data" => {
                let Some(value) = take_value(args, &mut index) else {
                    return Parsed::Error(format!("{arg} needs a value."));
                };
                match arg {
                    "--collection" => options.collection = Some(value),
                    "--folder" => options.folder_name = Some(value),
                    "--env" => options.environment = Some(value),
                    "--xunit" => options.xunit = Some(value),
                    _ => options.data_file = Some(value),
                }
            }
            "--tag" | "--exclude-tag" => {
                let Some(tag) = take_value(args, &mut index).filter(|t| !t.trim().is_empty()) else {
                    return Parsed::Error(format!("{arg} needs a value."));
                };
                if arg == "--tag" { options.tags.push(tag) } else { options.exclude_tags.push(tag) }
            }
            "--timeout" => match take_value(args, &mut index).and_then(|v| v.parse::<i32>().ok()) {
                Some(seconds) if seconds > 0 => options.timeout_seconds = seconds,
                _ => return Parsed::Error("--timeout needs a positive number of seconds.".to_string()),
            },
            "--var" => {
                let pair = take_value(args, &mut index).unwrap_or_default();
                match pair.find('=') {
                    Some(equals) if equals > 0 => {
                        options.variables.push(KeyValue::new(&pair[..equals], &pair[equals + 1..]));
                    }
                    _ => return Parsed::Error("--var needs name=value.".to_string()),
                }
            }
            _ if arg.starts_with('-') => {
                return Parsed::Error(format!("Unknown option \"{arg}\". Try: ricochet --help"));
            }
            _ => {
                positional += 1;
                if positional > 1 {
                    return Parsed::Error(format!("Unexpected argument \"{arg}\". Try: ricochet --help"));
                }
                options.folder = arg.to_string();
            }
        }
        index += 1;
    }

    Parsed::Run(options)
}

/// Moves to the value after an option and returns it, or None when the command line ends there.
fn take_value(args: &[String], index: &mut usize) -> Option<String> {
    *index += 1;
    args.get(*index).cloned()
}

/// Runs the `run` command and returns its exit code.
pub fn run_command(args: &[String]) -> i32 {
    match parse_args(args) {
        Parsed::Run(options) => execute(&options),
        Parsed::Help => {
            println!("{RUN_USAGE}");
            EXIT_PASSED
        }
        Parsed::Error(message) => cannot_run(message),
    }
}

fn execute(options: &RunArgs) -> i32 {
    // Cookies would need a jar in the sender, which the Rust port does not have yet. Refusing is safer than a run
    // that quietly differs from the app's behaviour.
    if options.cookies {
        return cannot_run("--cookies is not ported to the Rust CLI yet, so cookies would not be kept. Run without it, or use the C# CLI.");
    }

    let root = match std::path::absolute(&options.folder) {
        Ok(path) => path,
        Err(error) => return cannot_run(format!("Could not work out the folder \"{}\": {error}", options.folder)),
    };
    if !looks_like_workspace(&root) {
        return cannot_run(format!("\"{}\" is not a Ricochet workspace folder.", root.display()));
    }

    // Read before the Workspace, so a bad data file fails fast.
    let data_rows = match options.data_file.as_deref().map(read_data_rows).transpose() {
        Ok(rows) => rows,
        Err(message) => return cannot_run(message),
    };

    let mut folder = WorkspaceFolder::new(&root);
    let (workspace, issues) = match folder.load() {
        Ok(loaded) => loaded,
        Err(error) => return cannot_run(format!("Could not read the workspace folder: {error}")),
    };

    // An unreadable file could be hiding tests, so a green result wouldn't mean anything.
    if !issues.is_empty() {
        for issue in &issues {
            eprintln!("{}: {}", issue.path, issue.problem);
        }
        return cannot_run("Some workspace files could not be used. Fix them and run again.");
    }

    // Lowest priority first: .env, then RICOCHET_VAR_ variables, then --var. Later entries win.
    let dot_env = match dotenv::read(&root) {
        Ok(read) => read,
        Err(error) => return cannot_run(format!("Could not read {}: {error}", dotenv::FILE_NAME)),
    };
    for line in &dot_env.skipped_lines {
        eprintln!("Warning: {} line {line} was skipped, because it is not KEY=VALUE.", dotenv::FILE_NAME);
    }

    // Certificate checks are a CI setting of their own. The app's Settings page does not reach the CLI (ADR 0007).
    if options.insecure {
        eprintln!("Warning: TLS certificate checks are off (--insecure). Requests are not verified.");
    }

    let mut sender = RequestSender::new();
    let settings = SenderSettings {
        timeout: Duration::from_secs(u64::try_from(options.timeout_seconds).unwrap_or(120)),
        verify_certificates: !options.insecure,
        ..SenderSettings::default()
    };
    if let Some(problem) = sender.configure(settings) {
        return cannot_run(problem);
    }

    let mut overrides: Vec<KeyValue> = Vec::new();
    overrides.extend(dot_env.variables);
    overrides.extend(variables_from_environment());
    overrides.extend(options.variables.iter().cloned());

    let run_options = RunOptions {
        collection_name: options.collection.clone(),
        folder_name: options.folder_name.clone(),
        environment_name: options.environment.clone(),
        overrides,
        stop_on_first_failure: options.bail,
        include_tags: folder_tags::normalize(Some(options.tags.clone())).unwrap_or_default(),
        exclude_tags: folder_tags::normalize(Some(options.exclude_tags.clone())).unwrap_or_default(),
    };

    let colors = Colors { enabled: io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none() };
    let started_at: DateTime<Local> = Local::now();

    let mut report: RunReport = match data_rows {
        None => {
            let mut printer = ResultPrinter::new(colors);
            let outcome = CollectionRunner::new(&sender).run_with(&workspace, &run_options, |result| printer.print(result));
            match outcome {
                Ok(report) => report,
                Err(error) => return cannot_run(error.message()),
            }
        }
        Some(rows) => {
            let outcome = data_rows::run_data_rows(&sender, &workspace, &run_options, &rows, |row| {
                print_row(row, rows.len(), colors);
            });
            let row_reports = match outcome {
                Ok(reports) => reports,
                Err(error) => return cannot_run(error.message()),
            };

            let stopped_early = options.bail && row_reports.len() < rows.len();
            let combined = data_rows::combine(&row_reports, stopped_early);

            let passed = row_reports.iter().filter(|r| r.report.passed()).count();
            println!();
            println!("Data rows: {} run, {passed} passed, {} failed.", row_reports.len(), row_reports.len() - passed);
            combined
        }
    };

    report.certificates_unverified = options.insecure;
    println!();
    println!("{}", collection_runner::summary(&report));
    for caution in collection_runner::cautions(&report) {
        println!("Note: {caution}");
    }

    if let Some(path) = &options.xunit {
        if let Err(error) = fs::write(path, xunit::write_xunit(&report, started_at)) {
            return cannot_run(format!("Could not write the xUnit report: {error}"));
        }
    }

    if report.passed() { EXIT_PASSED } else { EXIT_TESTS_FAILED }
}

/// Prints one row's results under its number. Each row's results start a new collection header, as in the C# CLI.
fn print_row(row: &DataRowReport, row_count: usize, colors: Colors) {
    println!();
    println!("Row {} of {row_count}  {}", row.row, colors.verdict(row.report.passed()));

    let mut printer = ResultPrinter::new(colors);
    for result in &row.report.results {
        printer.print(result);
    }
    println!("  {}", collection_runner::summary(&row.report));
}

/// Reads and parses a data file. Its values are never printed, so the message names only the file.
fn read_data_rows(path: &str) -> Result<Vec<Row>, String> {
    let format = data_file::format_for(path).map_err(|e| e.message().to_string())?;

    let bytes = fs::read(path).map_err(|error| format!("Could not read the data file \"{path}\": {error}"))?;
    // Like File.ReadAllText, a byte order mark is dropped and invalid UTF-8 is replaced rather than failing.
    let text = String::from_utf8_lossy(&bytes);
    let text = text.trim_start_matches('\u{feff}');

    let rows = data_file::parse(text, format).map_err(|e| e.message().to_string())?;
    if rows.is_empty() {
        return Err(format!("The data file \"{path}\" has no rows to run."));
    }
    Ok(rows)
}

/// Variables from `RICOCHET_VAR_<name>` environment variables. The prefix is matched without regard to case.
fn variables_from_environment() -> Vec<KeyValue> {
    std::env::vars_os()
        .filter_map(|(name, value)| {
            let name = name.to_string_lossy().into_owned();
            let value = value.to_string_lossy().into_owned();
            let prefix_matches = name.get(..VARIABLE_PREFIX.len()).is_some_and(|p| p.eq_ignore_ascii_case(VARIABLE_PREFIX));
            (prefix_matches && name.len() > VARIABLE_PREFIX.len()).then(|| KeyValue::new(&name[VARIABLE_PREFIX.len()..], value))
        })
        .collect()
}

/// A folder is a workspace when it has the marker file or a collections folder, as `WorkspaceFolder.LooksLikeWorkspace`.
fn looks_like_workspace(root: &Path) -> bool {
    root.join(MARKER_FILE_NAME).is_file() || root.join(COLLECTIONS_DIR).is_dir()
}

fn cannot_run(message: impl AsRef<str>) -> i32 {
    eprintln!("{}", message.as_ref());
    EXIT_CANNOT_RUN
}

#[derive(Debug, Clone, Copy)]
pub struct Colors {
    pub enabled: bool,
}

impl Colors {
    /// PASS or FAIL, in green or red when colors are on.
    pub fn verdict(self, passed: bool) -> String {
        let word = if passed { "PASS" } else { "FAIL" };
        match (self.enabled, passed) {
            (false, _) => word.to_string(),
            (true, true) => format!("\u{1b}[32m{word}\u{1b}[0m"),
            (true, false) => format!("\u{1b}[31m{word}\u{1b}[0m"),
        }
    }
}

/// Prints the results of one run. A collection's name is printed when it changes, as in the C# CLI.
pub struct ResultPrinter {
    colors: Colors,
    collection: Option<String>,
}

impl ResultPrinter {
    pub fn new(colors: Colors) -> Self {
        Self { colors, collection: None }
    }

    pub fn print(&mut self, result: &RequestRunResult) {
        if self.collection.as_deref() != Some(result.collection.as_str()) {
            self.collection = Some(result.collection.clone());
            println!("{}", result.collection);
        }

        let status = match result.status_code {
            Some(code) => format!("  {code}  {} ms", result.elapsed_ms),
            None => String::new(),
        };
        println!("  {}  {} {}{status}", self.colors.verdict(result.passed()), result.method, result.path);

        if let Some(error) = &result.error {
            println!("        {error}");
        }

        for assertion in &result.assertions {
            let detail = if assertion.passed { String::new() } else { format!(": {}", assertion.message) };
            println!("        {}  {}{detail}", self.colors.verdict(assertion.passed), assertion.description);
        }

        // Only the names are printed. Captured values are never shown.
        for name in &result.captured {
            println!("        SET   {name}");
        }
    }
}
