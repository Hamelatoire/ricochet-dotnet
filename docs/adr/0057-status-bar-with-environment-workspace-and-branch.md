---
status: proposed
---

# A status bar with the Environment, Workspace and Git branch

The window doesn't show which Environment is active, which Workspace is open, or which Git branch its files are on. A request sent to the wrong server, or a file changed on the wrong branch, is easy to miss because nothing on screen says what the app is pointing at. We will add a thin status bar along the bottom of the window showing the active Environment, the Workspace folder and, when the folder is a Git repository, the current branch. Each item opens the matching control when clicked.

## Decisions within this

- **Show the three things that change what gets sent or saved:** the Environment decides the server, the Workspace decides the files, and the branch decides what a commit will contain.
- **Read the branch locally, with no network:** the branch name is read from the repository on disk, which [ADR 0011](0011-read-only-git-status.md) already allows, so nothing is fetched.
- **Click opens the control:** the Environment item opens the switcher, the branch item shows the read-only status from [ADR 0011](0011-read-only-git-status.md), and the Workspace item opens the folder.
- **Hidden when there is nothing to show:** a Workspace without Git shows no branch, rather than a placeholder.

## Considered Options

- **No status bar:** no extra space, but the active Environment stays hidden in a menu.
- **Show the Environment in the title bar only:** visible, but it crowds the title and doesn't show the branch.
- **Show a full path instead of the folder name:** precise, but a long path takes the bar's width and is seldom needed.

## Consequences

- The bar uses one line of height, so it must be checked against the sizes in [ADR 0042](0042-denser-layout-and-larger-controls.md).
- The branch is read when the window gains focus, as today, so a branch changed outside the app shows once the app is focused again.
- The bar must not repeat the send state, which the progress display in [ADR 0049](0049-show-elapsed-time-while-a-request-is-in-flight.md) covers.
