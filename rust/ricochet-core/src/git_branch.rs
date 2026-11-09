//! Port of the branch part of `GitStatusReader.cs` (ADR 0011): the current branch of the Workspace's git repository.
//!
//! It runs only `rev-parse`, which reads refs and never changes anything, and it never fetches. It never prompts for
//! a password, and it does not take git's optional locks. The git program is found on PATH, skipping any PATH entry
//! inside the Workspace folder, so a repository can't supply its own git (see `git_clone::find_git`).
//!
//! Returns None when git is not installed, the folder is not a repository (or has no commits yet), or git does not
//! answer within three seconds. For a detached HEAD it returns the short commit hash, as the C# does.
//!
//! Not ported yet: the ahead and behind counts (`git_status::status_from_output` already parses them), and the
//! kill of a git process's whole tree on timeout: only the git process itself is killed here.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::git_clone::find_git;

/// The total time all git commands together may take (GitStatusReader.Limit).
const LIMIT: Duration = Duration::from_secs(3);

/// Stops a console window from opening for git on Windows (CreateNoWindow in the C# version).
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// The current branch of the repository at `folder`, or the short commit hash when HEAD is detached.
/// None when git can't give one (see the module docs).
pub fn current_branch(folder: &Path) -> Option<String> {
    let git = find_git(folder)?;
    read_branch(&git, folder)
}

/// Reads the branch with the git program at `git`. Split out from [`current_branch`] so the lookup can be tested
/// with a known program, and so a missing program gives None.
pub fn read_branch(git: &Path, folder: &Path) -> Option<String> {
    let deadline = Instant::now() + LIMIT;

    let branch = run_git(git, folder, deadline, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    if branch == "HEAD" {
        return run_git(git, folder, deadline, &["rev-parse", "--short", "HEAD"]);
    }

    Some(branch)
}

/// Runs one git command in `folder` and returns its trimmed stdout. None when it fails, prints nothing, or runs past
/// `deadline`, in which case the process is killed.
fn run_git(git: &Path, folder: &Path, deadline: Instant, arguments: &[&str]) -> Option<String> {
    let mut command = Command::new(git);
    command
        .current_dir(folder)
        .args(arguments)
        // Never prompt for a password, and never take a lock that could get in the user's way.
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = command.spawn().ok()?;

    // Read stdout on its own thread so git can't block on a full pipe while we wait for it to exit.
    let mut stdout_pipe = child.stdout.take()?;
    let reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stdout_pipe.read_to_end(&mut bytes);
        String::from_utf8_lossy(&bytes).into_owned()
    });

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let output = reader.join().unwrap_or_default();
                if !status.success() {
                    return None;
                }
                let text = output.trim();
                return (!text.is_empty()).then(|| text.to_string());
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    // The reader is not joined: a helper process could still hold the pipe open.
                    return None;
                }
                thread::sleep(Duration::from_millis(20));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}
