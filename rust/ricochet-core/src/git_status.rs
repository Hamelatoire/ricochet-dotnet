//! Port of `GitStatusReader.cs` (ADR 0011, ADR 0057): the branch of the Workspace's git repository and how far it
//! is from its upstream. Counts compare with what git last fetched, so they can be out of date.
//!
//! This module is the pure parsing half. It takes the output of the git commands the C# reader runs and builds the
//! status from it, so it can be tested with fixture strings and never runs git. Not ported yet: finding the git
//! program on PATH (`FindGit`), running the commands with the safety environment (`GIT_TERMINAL_PROMPT=0`,
//! `GIT_OPTIONAL_LOCKS=0`), and the three-second limit.

/// The branch of a git repository, and how far it is from its upstream.
///
/// `branch` is the branch name, or the short commit hash when HEAD is detached. `ahead` and `behind` are `None`
/// when there is no upstream, or when HEAD is detached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitStatus {
    pub branch: String,
    /// Commits on this branch that the upstream doesn't have.
    pub ahead: Option<i32>,
    /// Commits on the upstream that this branch doesn't have.
    pub behind: Option<i32>,
}

/// Builds the status from the output of the git commands the C# reader runs, in the same order.
///
/// - `head`: output of `rev-parse --abbrev-ref HEAD`. `None` when git failed.
/// - `short_head`: output of `rev-parse --short HEAD`. Only read when `head` is `HEAD` (detached).
/// - `counts`: output of `rev-list --left-right --count @{upstream}...HEAD`. Only read when `head` is a branch.
///
/// Returns `None` when git failed, as the C# does. Otherwise a broken or missing count gives counts of `None`,
/// and the branch is still shown.
pub fn status_from_output(head: Option<&str>, short_head: Option<&str>, counts: Option<&str>) -> Option<GitStatus> {
    let branch = head?.trim();

    if branch == "HEAD" {
        let hash = short_head?.trim();
        return Some(GitStatus { branch: hash.to_string(), ahead: None, behind: None });
    }

    match counts.and_then(parse_counts) {
        Some((behind, ahead)) => Some(GitStatus { branch: branch.to_string(), ahead: Some(ahead), behind: Some(behind) }),
        None => Some(GitStatus { branch: branch.to_string(), ahead: None, behind: None }),
    }
}

/// Parses `behind<TAB>ahead` from `rev-list --left-right --count`, returning `(behind, ahead)`.
///
/// The left side counts the upstream's commits that this branch lacks, so it is `behind`. Each count must be only
/// ASCII digits (no sign, no spaces) and fit in an `i32`, as `int.TryParse` with `NumberStyles.None` requires.
pub fn parse_counts(output: &str) -> Option<(i32, i32)> {
    let parts: Vec<&str> = output.trim().split('\t').filter(|part| !part.is_empty()).collect();
    let [behind, ahead] = parts.as_slice() else {
        return None;
    };

    Some((parse_digits(behind)?, parse_digits(ahead)?))
}

fn parse_digits(text: &str) -> Option<i32> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }

    text.parse::<i32>().ok()
}
