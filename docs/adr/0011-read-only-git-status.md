---
status: accepted (built; the sidebar refreshes on window activation, not on every outside change)
---

# Show git status read-only, using the installed git

Git is how people share a Workspace ([ADR 0003](0003-git-friendly-workspace-files.md)), but the app can't currently show which branch the requests came from, or whether there is work to push or pull. We will show the branch and the commits ahead and behind at the bottom of the sidebar, by running the `git` already installed on the machine. The app won't commit, pull, push or fetch: those stay in the user's own git tools, because a poor merge or push tool would be worse than none, and network activity needs the user's consent ([ADR 0005](0005-local-first.md)).

## Decisions within this

- **Never fetch by itself:** the ahead and behind counts compare against what git last fetched, so they can be out of date, and the tooltip says so. Showing fresher numbers would mean background traffic, which [ADR 0005](0005-local-first.md) doesn't allow.
- **Only commands that can't run the repository's code:** a folder someone downloaded can carry a `.git/config` that runs a program during `git status`, through settings such as `core.fsmonitor`. So the app reads only the branch and commit counts, with `rev-parse`, `rev-list` and similar commands, and doesn't show uncommitted changes.
- **Find git on `PATH`, never in the Workspace folder:** a `git.exe` inside a shared folder must not be run.
- **Stay quiet when it doesn't apply:** with no git installed, or a Workspace outside a repository, nothing is shown. A detached HEAD shows the short commit hash.
- **Never block the interface:** git runs in the background with a time limit. If it fails or times out, the status is hidden.

## Considered Options

- **LibGit2Sharp:** needs no installed git, but adds a native library to every platform's build, needs checking under Native AOT, and doesn't read everything in a user's git setup.
- **Read the `.git` folder directly:** no process or dependency, and enough for the branch name, but counting commits means understanding packed objects, which is not worth writing again.
- **Full git features in the app:** convenient for people who don't use git, but it adds credential handling, conflict handling and network traffic to a tool meant to stay small.

## Consequences

- People who want to commit still leave the app. If that proves a real need, commit and push can be a later decision, as long as they only run when the user asks.
- A status bar can't show uncommitted changes without a safe way to get them. Comparing against the files the app itself wrote could be considered later.
- The status must refresh when the branch changes. The file watcher deliberately treats `.git` activity differently, so it needs checking that a checkout with no changed files still updates it.
