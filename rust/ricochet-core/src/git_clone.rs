//! Port of src/Ricochet.Core/GitCloner.cs (ADR 0034). Clones a git repository into an empty folder the user
//! chose, using the git already installed. It runs only `git clone`: it never commits, pushes or stores
//! credentials, and it never prompts for a password. Authentication uses the user's existing git setup, so a
//! URL that carries a password is refused.

use std::fs;
use std::io::Read;
use std::path::{MAIN_SEPARATOR, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

/// Only transports a person expects. Without this list git's ext:: transport could run an arbitrary program.
const ALLOWED_PROTOCOLS: [&str; 5] = ["file", "http", "https", "ssh", "git"];

/// The marker file and folder that make a folder a Ricochet workspace (WorkspaceFolder.cs).
const MARKER_FILE_NAME: &str = "ricochet.json";
const COLLECTIONS_DIR: &str = "collections";

/// Stops a console window from opening for git on Windows (CreateNoWindow in the C# version).
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// What a clone produced: whether git succeeded, whether the result is a Ricochet workspace, and a message to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitCloneResult {
    pub succeeded: bool,
    pub is_workspace: bool,
    pub message: String,
}

impl GitCloneResult {
    fn failed(message: impl Into<String>) -> Self {
        Self { succeeded: false, is_workspace: false, message: message.into() }
    }
}

/// None when the folder can take a clone, otherwise the reason it can't. It must exist and be empty.
pub fn check_target_folder(folder: &str) -> Option<String> {
    if folder.trim().is_empty() {
        return Some("Choose a folder for the clone.".into());
    }

    let path = Path::new(folder);
    if path.is_file() {
        return Some("That path is a file. Choose an empty folder.".into());
    }
    if !path.is_dir() {
        return Some("That folder does not exist. Create it, then choose it.".into());
    }

    match fs::read_dir(path) {
        Err(error) => Some(format!("That folder can't be read: {error}.")),
        Ok(mut entries) => {
            if entries.next().is_some() {
                Some("That folder is not empty. Choose an empty folder so the clone doesn't mix with existing files.".into())
            } else {
                None
            }
        }
    }
}

/// None when the URL can be cloned, otherwise the reason it can't.
pub fn check_url(url: &str) -> Option<String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Some("Enter the address of a git repository.".into());
    }
    if trimmed.starts_with('-') {
        return Some("The address can't start with a dash.".into());
    }

    if let Some((scheme, rest)) = absolute_scheme(trimmed) {
        if user_info(rest).is_some_and(|info| info.contains(':')) {
            return Some("The address contains a password. Keep passwords in your git setup, not in the address.".into());
        }

        let scheme = scheme.to_ascii_lowercase();
        if !ALLOWED_PROTOCOLS.contains(&scheme.as_str()) {
            return Some(format!("Ricochet clones over {} only.", ALLOWED_PROTOCOLS.join(", ")));
        }
    }

    None
}

/// Clones `url` into `folder` and reports whether the result is a Ricochet workspace. A result that is not a
/// workspace stays on disk, untouched by Ricochet, so the user can look at it.
///
/// This blocks until git exits, so call it from a background thread. Setting `cancel` kills git and returns a
/// failed result.
pub fn clone_repository(url: &str, folder: &str, cancel: &AtomicBool) -> GitCloneResult {
    if let Some(problem) = check_url(url) {
        return GitCloneResult::failed(problem);
    }
    if let Some(problem) = check_target_folder(folder) {
        return GitCloneResult::failed(problem);
    }

    let Some(git) = find_git(Path::new(folder)) else {
        return GitCloneResult::failed("git was not found on PATH. Install git to clone a repository.");
    };

    let target = absolute(Path::new(folder));
    let (exit_code, stderr) = run_clone(&git, &target, url.trim(), cancel);
    if exit_code != 0 {
        let last_line = stderr.lines().map(str::trim).filter(|line| !line.is_empty()).last();
        return GitCloneResult::failed(match last_line {
            None => "git clone failed.".to_string(),
            Some(line) => format!("git clone failed: {line}"),
        });
    }

    if looks_like_workspace(&target) {
        GitCloneResult { succeeded: true, is_workspace: true, message: "Cloned. The folder is a Ricochet workspace.".into() }
    } else {
        GitCloneResult {
            succeeded: true,
            is_workspace: false,
            message: "Cloned, but the folder is not a Ricochet workspace: it has no ricochet.json and no collections folder.".into(),
        }
    }
}

/// Splits `scheme:rest` the way a URI does. Returns None for scp-style addresses such as `git@host:path`, and for
/// Windows drive letters such as `C:\repos`, because a one-letter scheme isn't treated as a scheme.
fn absolute_scheme(text: &str) -> Option<(&str, &str)> {
    let colon = text.find(':')?;
    let scheme = &text[..colon];

    let mut chars = scheme.chars();
    let first_is_letter = chars.next().is_some_and(|c| c.is_ascii_alphabetic());
    let rest_is_valid = chars.all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.');
    if scheme.len() <= 1 || !first_is_letter || !rest_is_valid {
        return None;
    }

    Some((scheme, &text[colon + 1..]))
}

/// The user-info part of an authority (`user:password` in `https://user:password@host/...`), if there is one.
fn user_info(rest: &str) -> Option<&str> {
    let authority_and_path = rest.strip_prefix("//")?;
    let end = authority_and_path.find(['/', '?', '#']).unwrap_or(authority_and_path.len());
    let authority = &authority_and_path[..end];
    authority.rfind('@').map(|at| &authority[..at])
}

/// Git's own lookup, the same one ADR 0011's status reader uses (GitStatusReader.FindGit). It skips PATH entries
/// inside the workspace so a repository can't supply its own git.
fn find_git(folder: &Path) -> Option<PathBuf> {
    let names: &[&str] = if cfg!(windows) { &["git.exe"] } else { &["git"] };
    let workspace = with_separator(&absolute(folder));

    for directory in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
        if !directory.is_absolute() {
            continue;
        }
        if is_within(&with_separator(&directory), &workspace) {
            continue;
        }
        for name in names {
            let candidate = directory.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    None
}

/// Runs `git clone`. Returns the exit code and the stderr text. Only stderr is kept, because stdout is discarded.
fn run_clone(git: &Path, target: &Path, url: &str, cancel: &AtomicBool) -> (i32, String) {
    let mut command = Command::new(git);
    command
        .current_dir(target)
        // "--" stops a URL that looks like an option from being read as one.
        .args(["clone", "--quiet", "--"])
        .arg(url)
        .arg(target)
        // Never prompt for a password, and allow only the transports in ALLOWED_PROTOCOLS.
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ALLOW_PROTOCOL", ALLOWED_PROTOCOLS.join(":"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => return (-1, "git could not be started.".into()),
    };

    // Read stderr on its own thread so git can't block on a full pipe while we wait for it to exit.
    let mut stderr_pipe = child.stderr.take().expect("stderr was piped");
    let reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stderr_pipe.read_to_end(&mut bytes);
        String::from_utf8_lossy(&bytes).into_owned()
    });

    loop {
        if cancel.load(Ordering::SeqCst) {
            let _ = child.kill();
            let _ = child.wait();
            // The reader is not joined here: a helper process that git started could still hold the pipe open.
            return (-1, "The clone was cancelled.".into());
        }

        match child.try_wait() {
            Ok(Some(status)) => {
                let stderr = reader.join().unwrap_or_default();
                return (status.code().unwrap_or(-1), stderr);
            }
            Ok(None) => thread::sleep(Duration::from_millis(50)),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return (-1, "git could not be waited on.".into());
            }
        }
    }
}

/// Mirrors WorkspaceFolder.LooksLikeWorkspace: the marker file exists, or the collections folder does.
fn looks_like_workspace(path: &Path) -> bool {
    path.join(MARKER_FILE_NAME).is_file() || path.join(COLLECTIONS_DIR).is_dir()
}

fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

/// The path with exactly one trailing separator, so that `C:\repo` doesn't match `C:\repo2`.
fn with_separator(path: &Path) -> String {
    let mut text = path.to_string_lossy().trim_end_matches(MAIN_SEPARATOR).to_string();
    text.push(MAIN_SEPARATOR);
    text
}

/// Case-insensitive on Windows, as the C# version's OrdinalIgnoreCase is.
fn is_within(directory: &str, workspace: &str) -> bool {
    if cfg!(windows) {
        directory.to_ascii_lowercase().starts_with(&workspace.to_ascii_lowercase())
    } else {
        directory.starts_with(workspace)
    }
}
