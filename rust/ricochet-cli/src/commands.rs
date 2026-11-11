//! The `export`, `env`, `docs`, `zip` and `clone` commands, ported from `ExportCommand`, `EnvCommand`, `DocsCommand`,
//! `ZipCommand` and `CloneCommand` in src/Ricochet.Cli/Program.cs. Messages and exit codes match the C# CLI.
//! Secret values are never written by any of them (ADR 0004, ADR 0036).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use ricochet_core::collection_runner::{EXIT_CANNOT_RUN, EXIT_PASSED, EXIT_TESTS_FAILED};
use ricochet_core::models::Workspace;
use ricochet_core::personal_state::default_personal_directory;
use ricochet_core::secrets::{self, SecretValue};
use ricochet_core::store::{WorkspaceFolder, looks_like_workspace};
use ricochet_core::{
    docs_html, environment_transfer, export_files, git_clone, openapi_export, postman_export, workspace_archive,
    workspace_secrets,
};

/// An import refused because of a name clash. Same code as a failed check.
const EXIT_REFUSED: i32 = 1;

pub fn cannot_run(message: impl AsRef<str>) -> i32 {
    eprintln!("{}", message.as_ref());
    EXIT_CANNOT_RUN
}

fn full_path(path: &str) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| PathBuf::from(path))
}

fn not_a_workspace(folder: &str) -> i32 {
    cannot_run(format!("\"{}\" is not a Ricochet workspace folder.", full_path(folder).display()))
}

/// Loads a Workspace folder. Unreadable files are printed and refused, because they could hide part of what the
/// command works on. `again` finishes the message, such as "export again".
fn load_workspace(folder_path: &str, again: &str) -> Result<(WorkspaceFolder, Workspace), i32> {
    if !looks_like_workspace(Path::new(folder_path)) {
        return Err(not_a_workspace(folder_path));
    }
    let mut folder = WorkspaceFolder::new(full_path(folder_path));
    let (workspace, issues) = folder
        .load()
        .map_err(|error| cannot_run(format!("Could not read the workspace folder: {error}")))?;
    if !issues.is_empty() {
        for issue in &issues {
            eprintln!("{}: {}", issue.path, issue.problem);
        }
        return Err(cannot_run(format!("Some workspace files could not be used. Fix them and {again}.")));
    }
    Ok((folder, workspace))
}

/// This machine's Secrets. On CI there usually are none, and an unreadable vault counts as empty, as in the C# CLI.
pub fn load_secrets() -> BTreeMap<String, SecretValue> {
    default_personal_directory()
        .and_then(|personal| secrets::default_vault(&personal))
        .and_then(|vault| vault.load().ok())
        .unwrap_or_default()
}

/// The Workspace with this machine's Secret values put back in.
pub fn with_secrets(workspace: &Workspace, folder: &WorkspaceFolder) -> Workspace {
    workspace_secrets::merge(workspace, &workspace_secrets::for_workspace(&load_secrets(), folder.id()))
}

// ---- export ----

pub const EXPORT_USAGE: &str = "Ricochet: export a collection to a Postman v2.1 file, an OpenAPI 3 file, or both.

Usage:
  ricochet export <collection> [options]

Arguments:
  collection             The name of the collection to export.

Options:
  --postman <file>       Write a Postman v2.1 collection to this file.
  --openapi <file>       Write an OpenAPI 3 document to this file.
  --workspace <folder>   The workspace folder. Defaults to the current folder.
  --force                Overwrite a file that already exists.
  --help                 Show this help.

Secret values are never written. Secret Variables are left out, and a secret's value is
replaced with {{name}}. Everything else that cannot be exported is listed after the export.
Files are never written inside the workspace folder.

Exit codes:
  0  The file was written.
  2  The export could not happen, for example because the collection does not exist,
     a file already exists without --force, or the workspace is unreadable.";

#[derive(Debug, Default, PartialEq)]
pub struct ExportArgs {
    pub collection: String,
    pub workspace: String,
    pub postman: Option<String>,
    pub openapi: Option<String>,
    pub force: bool,
}

/// Ok(None) means help was shown.
pub fn parse_export(args: &[String]) -> Result<Option<ExportArgs>, String> {
    let mut options = ExportArgs { workspace: ".".into(), ..ExportArgs::default() };
    let mut collection = None;
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        let mut value = || {
            index += 1;
            args.get(index).cloned()
        };
        match arg {
            "--help" | "-h" => return Ok(None),
            "--force" => options.force = true,
            "--workspace" => {
                options.workspace = value().unwrap_or_default();
                if options.workspace.is_empty() {
                    return Err("--workspace needs a folder.".into());
                }
            }
            "--postman" => {
                options.postman = Some(value().filter(|v| !v.trim().is_empty()).ok_or("--postman needs a file.")?);
            }
            "--openapi" => {
                options.openapi = Some(value().filter(|v| !v.trim().is_empty()).ok_or("--openapi needs a file.")?);
            }
            _ if arg.starts_with('-') => return Err(format!("Unknown option \"{arg}\". Try: ricochet export --help")),
            _ => {
                if collection.is_some() {
                    return Err(format!("Unexpected argument \"{arg}\". Try: ricochet export --help"));
                }
                collection = Some(arg.to_string());
            }
        }
        index += 1;
    }

    options.collection = collection.ok_or("Name the collection to export. Try: ricochet export --help")?;
    if options.postman.is_none() && options.openapi.is_none() {
        return Err("Choose at least one format: --postman <file> or --openapi <file>.".into());
    }
    Ok(Some(options))
}

pub fn export_command(args: &[String]) -> i32 {
    let options = match parse_export(args) {
        Ok(Some(options)) => options,
        Ok(None) => {
            println!("{EXPORT_USAGE}");
            return EXIT_PASSED;
        }
        Err(message) => return cannot_run(message),
    };

    let (folder, workspace) = match load_workspace(&options.workspace, "export again") {
        Ok(loaded) => loaded,
        Err(code) => return code,
    };
    // Secret values come from this machine's vault, so they can be replaced with their placeholders.
    let workspace = with_secrets(&workspace, &folder);
    let collection = match export_files::find_collection(&workspace.collections, &options.collection) {
        Ok(collection) => collection,
        Err(error) => return cannot_run(error.0),
    };

    // Check every target before writing any, so a refused second file never leaves the first one behind.
    let mut targets: Vec<(&str, PathBuf, bool)> = Vec::new();
    for (label, path, postman) in [("Postman", &options.postman, true), ("OpenAPI", &options.openapi, false)] {
        if let Some(path) = path {
            match export_files::check(Path::new(path), folder.root(), options.force) {
                Ok(full) => targets.push((label, full, postman)),
                Err(error) => return cannot_run(error.0),
            }
        }
    }
    if let [first, second] = targets.as_slice() {
        if first.1.to_string_lossy().eq_ignore_ascii_case(&second.1.to_string_lossy()) {
            return cannot_run("--postman and --openapi need different files.");
        }
    }

    for (label, path, postman) in targets {
        let (json, report, empty) = if postman {
            let result = postman_export::export(collection);
            (result.json.clone(), result.to_report_text(), result.notes.is_empty())
        } else {
            let result = openapi_export::export(collection);
            (result.json.clone(), result.report_text(), result.notes.is_empty())
        };
        if let Err(error) = export_files::write(&path, &json, folder.root(), options.force) {
            return cannot_run(error.0);
        }
        println!("Wrote {label} export to {}", path.display());
        println!("{}", if empty { "Nothing was left out." } else { report.as_str() });
        println!();
    }
    EXIT_PASSED
}

// ---- docs ----

pub const DOCS_USAGE: &str = "Ricochet: write one collection as a self-contained HTML page for people who do not use Ricochet (ADR 0028).

Usage:
  ricochet docs <collection> <file.html> [options]

Arguments:
  collection             The name of the collection to document.
  file.html              The page to write. It must be outside the workspace folder.

Options:
  --workspace <folder>   The workspace folder. Defaults to the current folder.
  --force                Overwrite a file that already exists.
  --help                 Show this help.

Secret values are shown as ***. The page has no scripts and makes no network calls.

Exit codes:
  0  The page was written.
  2  The page could not be written, for example because the collection does not exist,
     a file already exists without --force, or the workspace is unreadable.";

pub fn docs_command(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{DOCS_USAGE}");
        return EXIT_PASSED;
    }

    let force = args.iter().any(|a| a == "--force");
    let mut workspace = ".".to_string();
    let mut positional = Vec::new();
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if arg == "--force" {
        } else if arg == "--workspace" {
            index += 1;
            match args.get(index) {
                Some(value) => workspace = value.clone(),
                None => return cannot_run("--workspace needs a folder."),
            }
        } else if arg.starts_with('-') {
            return cannot_run(format!("Unknown option \"{arg}\". Try: ricochet docs --help"));
        } else {
            positional.push(arg.clone());
        }
        index += 1;
    }

    let [name, file] = positional.as_slice() else {
        return cannot_run("ricochet docs needs a collection and a file. Try: ricochet docs --help");
    };

    let (folder, loaded) = match load_workspace(&workspace, "try again") {
        Ok(loaded) => loaded,
        Err(code) => return code,
    };

    let written = export_files::find_collection(&loaded.collections, name).and_then(|collection| {
        let target = export_files::check(Path::new(file), folder.root(), force)?;
        export_files::write(&target, &docs_html::render(collection), folder.root(), force)?;
        Ok((collection.name.clone(), target))
    });
    match written {
        Ok((name, target)) => {
            println!("Wrote documentation for {name} to {}", target.display());
            EXIT_PASSED
        }
        Err(error) => cannot_run(error.0),
    }
}

// ---- zip ----

pub const ZIP_USAGE: &str = "Ricochet: export a workspace to one ZIP file, or import such a ZIP into a new folder (ADR 0035).

Usage:
  ricochet zip export <workspace> <file.zip> [--force]
  ricochet zip import <file.zip> <new-folder>

Export includes only the shared workspace files. Personal state, History, Secrets and .env are
never included. The ZIP must be outside the workspace folder.

Import writes into a new or empty folder only, and never over an existing workspace. Every entry
is checked first, so a refused entry means nothing was written.

Options:
  --force                Overwrite a ZIP file that already exists (export only).
  --help                 Show this help.

Exit codes:
  0  The ZIP was written, or the workspace was imported.
  1  An entry was refused. Nothing was written.
  2  The command could not run, for example because a file is missing, the target folder is not
     empty, or a folder cannot be written.";

pub fn zip_command(args: &[String]) -> i32 {
    if args.is_empty() {
        println!("{ZIP_USAGE}");
        return EXIT_CANNOT_RUN;
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{ZIP_USAGE}");
        return EXIT_PASSED;
    }

    let verb = args[0].as_str();
    let mut rest: Vec<&String> = args[1..].iter().collect();
    // Like List.Remove in the C# CLI, only the first --force is taken out.
    let force = match rest.iter().position(|a| *a == "--force") {
        Some(at) => {
            rest.remove(at);
            true
        }
        None => false,
    };
    if let Some(option) = rest.iter().find(|a| a.starts_with('-')) {
        return cannot_run(format!("Unknown option \"{option}\". Try: ricochet zip --help"));
    }

    match (verb, rest.as_slice()) {
        ("export", [workspace, zip]) => zip_export(workspace, zip, force),
        ("import", [zip, folder]) if !force => zip_import(zip, folder),
        ("export", _) => cannot_run("ricochet zip export needs a workspace and a ZIP file. Try: ricochet zip --help"),
        ("import", _) => cannot_run("ricochet zip import needs a ZIP file and a new folder. Try: ricochet zip --help"),
        _ => cannot_run(format!("Unknown command \"zip {verb}\". Try: ricochet zip --help")),
    }
}

fn zip_export(workspace: &str, zip_path: &str, force: bool) -> i32 {
    if !looks_like_workspace(Path::new(workspace)) {
        return not_a_workspace(workspace);
    }
    let target = match export_files::check(Path::new(zip_path), Path::new(workspace), force) {
        Ok(target) => target,
        Err(error) => return cannot_run(error.0),
    };
    let parent = target.parent().map(Path::to_path_buf);
    if !parent.as_deref().is_some_and(can_write_to) {
        let shown = parent.map(|p| p.display().to_string()).unwrap_or_default();
        return cannot_run(format!("The ZIP cannot be written: \"{shown}\" is not a writable folder."));
    }

    // The Core export reads only the shared files and never loads Secrets, so none can end up in the ZIP.
    let result = workspace_archive::export(&full_path(workspace), &target);
    if !result.succeeded() {
        return zip_refused(&result.problems);
    }
    println!("Wrote {} workspace files to {}", result.files, target.display());
    println!("Personal state, History, Secrets and .env were left out.");
    EXIT_PASSED
}

fn zip_import(zip_path: &str, folder: &str) -> i32 {
    if !Path::new(zip_path).is_file() {
        return cannot_run(format!("There is no file at \"{}\".", full_path(zip_path).display()));
    }
    if let Err(problem) = workspace_archive::check_is_workspace(Path::new(zip_path)) {
        return cannot_run(problem);
    }

    let target = full_path(folder);
    if target.is_file() {
        return cannot_run(format!("\"{}\" is a file. Choose a new, empty folder.", target.display()));
    }
    if target.is_dir() && fs::read_dir(&target).map(|mut entries| entries.next().is_some()).unwrap_or(true) {
        return cannot_run(format!("\"{}\" is not empty. Choose a new, empty folder.", target.display()));
    }

    // Probe the nearest folder that already exists, without creating anything, so a refused import leaves nothing behind.
    let mut existing = Some(target.as_path());
    while let Some(path) = existing {
        if path.exists() {
            break;
        }
        existing = path.parent();
    }
    if !existing.is_some_and(|path| path.is_dir() && can_write_to(path)) {
        return cannot_run(format!("The workspace cannot be written to \"{}\".", target.display()));
    }

    let result = workspace_archive::import(Path::new(zip_path), &target);
    if !result.succeeded() {
        return zip_refused(&result.problems);
    }
    println!("Imported {} workspace files into {}", result.files, target.display());
    EXIT_PASSED
}

/// Creates and deletes a probe file, so an unwritable folder is reported before anything is written.
fn can_write_to(folder: &Path) -> bool {
    let probe = folder.join(format!(".ricochet-write-test-{}", ricochet_core::models::new_id()));
    fs::File::create(&probe).is_ok() && fs::remove_file(&probe).is_ok()
}

/// Core reports each refused entry and writes nothing when there is any, so exit 1 means nothing was written.
fn zip_refused(problems: &[String]) -> i32 {
    for problem in problems {
        eprintln!("{problem}");
    }
    eprintln!("Nothing was written.");
    EXIT_TESTS_FAILED
}

// ---- env ----

pub const ENV_USAGE: &str = "Ricochet: move one environment in or out of a file, without its secret values (ADR 0036).

Usage:
  ricochet env export <environment> <file.json> [--workspace <folder>] [--force]
  ricochet env import <file.json> [--workspace <folder>] [--replace]

Arguments:
  environment            The name of the environment to export.
  file.json              The file to write (export) or read (import).

Options:
  --workspace <folder>   The workspace folder. Defaults to the current folder.
  --force                Export: overwrite a file that already exists.
  --replace              Import: overwrite an environment with the same name. Without it, a name clash is refused.
  --help                 Show this help.

Secret values are never exported, and import never writes one. Secret variables come back without
a value, so set them again. A Secret this machine already has keeps its value when you replace.
Export files are never written inside the workspace folder.

Exit codes:
  0  The environment was exported or imported.
  1  The import was refused, because an environment with that name already exists and --replace was not given.
  2  The command could not run, for example because the environment does not exist, the file cannot be
     read or written, or the workspace is unreadable.";

#[derive(Debug, Default)]
struct EnvArgs {
    positional: Vec<String>,
    workspace: String,
    force: bool,
    replace: bool,
}

pub fn env_command(args: &[String]) -> i32 {
    if args.is_empty() || args[0] == "--help" || args[0] == "-h" {
        println!("{ENV_USAGE}");
        return if args.is_empty() { EXIT_CANNOT_RUN } else { EXIT_PASSED };
    }

    let mut options = EnvArgs { workspace: ".".into(), ..EnvArgs::default() };
    let mut index = 1;
    while index < args.len() {
        let arg = args[index].as_str();
        match arg {
            "--help" | "-h" => {
                println!("{ENV_USAGE}");
                return EXIT_PASSED;
            }
            "--force" => options.force = true,
            "--replace" => options.replace = true,
            "--workspace" => {
                index += 1;
                match args.get(index).filter(|v| !v.trim().is_empty()) {
                    Some(value) => options.workspace = value.clone(),
                    None => return cannot_run("--workspace needs a folder."),
                }
            }
            _ if arg.starts_with('-') => return cannot_run(format!("Unknown option \"{arg}\". Try: ricochet env --help")),
            _ => options.positional.push(arg.to_string()),
        }
        index += 1;
    }

    match args[0].as_str() {
        "export" => env_export(&options),
        "import" => env_import(&options),
        other => cannot_run(format!("Unknown command \"env {other}\". Try: ricochet env --help")),
    }
}

fn env_export(options: &EnvArgs) -> i32 {
    let [name, file] = options.positional.as_slice() else {
        return cannot_run("Usage: ricochet env export <environment> <file.json>. Try: ricochet env --help");
    };
    if options.replace {
        return cannot_run("--replace is for env import. Try: ricochet env --help");
    }

    let (folder, workspace) = match load_workspace(&options.workspace, "export again") {
        Ok(loaded) => loaded,
        Err(code) => return code,
    };
    // Merged only so the report can count the Secrets; the export itself never writes their values.
    let workspace = with_secrets(&workspace, &folder);
    let Some(environment) = environment_transfer::find_environment(&workspace.environments, name) else {
        let names: Vec<&str> = workspace.environments.iter().map(|e| e.name.as_str()).filter(|n| !n.is_empty()).collect();
        let known = if names.is_empty() {
            "The workspace has no environments.".to_string()
        } else {
            format!("Environments: {}.", names.join(", "))
        };
        return cannot_run(format!("No environment is named \"{name}\", or more than one has that name. {known}"));
    };

    if let Err(error) = export_files::write(Path::new(file), &environment_transfer::export(environment), folder.root(), options.force) {
        return cannot_run(error.0);
    }

    let left = environment_transfer::secret_count(environment);
    println!("Exported environment \"{}\" to {}", environment.name, full_path(file).display());
    if left == 0 {
        println!("No Secret values were left out.");
    } else {
        println!(
            "Left out {left} Secret {}. Secret values are never exported; set them again on the machine that imports this file.",
            if left == 1 { "value" } else { "values" }
        );
    }
    EXIT_PASSED
}

fn env_import(options: &EnvArgs) -> i32 {
    let [file] = options.positional.as_slice() else {
        return cannot_run("Usage: ricochet env import <file.json>. Try: ricochet env --help");
    };
    if options.force {
        return cannot_run("--force is for env export. Use --replace to overwrite an environment on import.");
    }

    let json = match fs::read(file) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).trim_start_matches('\u{feff}').to_string(),
        Err(error) => return cannot_run(format!("Could not read \"{}\": {error}", full_path(file).display())),
    };
    let file_name = Path::new(file).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let imported = match environment_transfer::import(&json, &file_name) {
        Ok(imported) => imported,
        Err(error) => return cannot_run(error.to_string()),
    };

    let (mut folder, workspace) = match load_workspace(&options.workspace, "import again") {
        Ok(loaded) => loaded,
        Err(code) => return code,
    };

    let incoming = imported.environment.clone();
    let clashed = workspace.environments.iter().any(|e| e.name.to_lowercase() == incoming.name.to_lowercase());
    let clash = if options.replace { environment_transfer::EnvironmentClash::Replace } else { environment_transfer::EnvironmentClash::Skip };
    let placement = environment_transfer::place(&workspace.environments, incoming.clone(), clash);
    if placement.placed.is_none() {
        eprintln!(
            "Environment \"{}\" already exists in this workspace, so nothing was imported. Pass --replace to overwrite it.",
            incoming.name
        );
        return EXIT_REFUSED;
    }

    // Split moves every Secret value out of the Workspace, and its values are dropped here rather than stored.
    // The vault is never written, so this machine's Secret values stay as they were.
    let updated = Workspace { environments: placement.environments, ..workspace };
    let (clean, _) = workspace_secrets::split(&updated);
    if let Err(error) = folder.save(&clean) {
        return cannot_run(format!("Could not save the workspace: {error}"));
    }

    let root = folder.root().display();
    if clashed {
        println!("Replaced environment \"{}\" in {root}", incoming.name);
    } else {
        println!("Imported environment \"{}\" into {root}", incoming.name);
    }
    println!("{}", imported.summary);
    for issue in &imported.issues {
        println!("Note: {}: {}", issue.category, issue.detail);
    }

    let secrets = environment_transfer::secret_count(&incoming);
    if secrets > 0 {
        println!(
            "Note: Secret values are never imported ({secrets} Secret {}). A value this machine already has is kept; set any others in the app.",
            if secrets == 1 { "variable" } else { "variables" }
        );
    }
    EXIT_PASSED
}

// ---- clone ----

pub const CLONE_USAGE: &str = "Ricochet: clone a git repository as a workspace (ADR 0034).

Usage:
  ricochet clone <git-url> <target-folder>

The target folder must already exist and be empty. Ricochet runs git clone only: it never
commits, pushes or stores credentials. Authentication uses your existing git setup, and a
password in the address is refused.

Exit codes:
  0  The clone succeeded and the folder is a Ricochet workspace.
  1  The clone succeeded, but the folder is not a Ricochet workspace. The files stay on disk.
  2  The clone could not run, for example because the target is not empty, git is not
     installed, or the clone failed.";

pub fn clone_command(args: &[String]) -> i32 {
    if args.is_empty() || args[0] == "--help" || args[0] == "-h" {
        println!("{CLONE_USAGE}");
        return if args.is_empty() { EXIT_CANNOT_RUN } else { EXIT_PASSED };
    }
    if args.len() != 2 || args.iter().any(|a| a.starts_with('-')) {
        return cannot_run("Usage: ricochet clone <git-url> <target-folder>. Try: ricochet clone --help");
    }

    // Ctrl+C ends the whole process group, git included, so nothing needs to set this.
    let cancel = AtomicBool::new(false);
    let result = git_clone::clone_repository(&args[0], &args[1], &cancel);
    if !result.succeeded {
        return cannot_run(result.message);
    }
    if !result.is_workspace {
        eprintln!("{}", result.message);
        return EXIT_TESTS_FAILED;
    }
    println!("{}", result.message);
    EXIT_PASSED
}
