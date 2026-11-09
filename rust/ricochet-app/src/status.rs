//! The status bar at the bottom of the window: the Environment, the Workspace folder and the Git branch (ADR 0057).

use std::path::Path;
use std::process::Command;

use ricochet_core::git_status::status_from_output;
use ricochet_core::models::Workspace;

/// Shown when no Environment is active.
pub const NO_ENVIRONMENT: &str = "No environment";

/// What the status bar shows. `branch` is None when the folder is not a Git repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusBar {
    pub environment: String,
    pub workspace: String,
    pub branch: Option<String>,
}

/// The name of the active Environment, or [`NO_ENVIRONMENT`] when none is set or it no longer exists.
pub fn environment_name(workspace: &Workspace) -> String {
    workspace
        .active_environment_id
        .as_deref()
        .and_then(|id| workspace.environments.iter().find(|environment| environment.id == id))
        .map(|environment| environment.name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| NO_ENVIRONMENT.to_string())
}

pub fn status_bar(workspace: &Workspace, folder: Option<&Path>, branch: Option<String>) -> StatusBar {
    StatusBar {
        environment: environment_name(workspace),
        workspace: folder.map(|path| path.display().to_string()).unwrap_or_else(|| "No workspace".to_string()),
        branch,
    }
}

/// The branch checked out in the Git repository at `folder`, read on disk with no network (ADR 0011).
/// None when the folder is not a repository or git cannot run.
pub fn read_branch(folder: &Path) -> Option<String> {
    let run = |args: &[&str]| -> Option<String> {
        let output = Command::new("git").arg("-C").arg(folder).args(args).output().ok()?;
        if !output.status.success() {
            return None;
        }
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    };
    let head = run(&["rev-parse", "--abbrev-ref", "HEAD"]);
    let short_head = match head.as_deref() {
        Some("HEAD") => run(&["rev-parse", "--short", "HEAD"]),
        _ => None,
    };
    status_from_output(head.as_deref(), short_head.as_deref(), None).map(|status| status.branch)
}
