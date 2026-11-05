# Ricochet

## Personal config

- Never commit personal config. This includes `.claude/` and `.vscode/` settings, `launch.json`, `.env` files, `*.user` files, local machine paths, personal email addresses and credentials.
- Stage files by exact path, not with `git add .` or `git add -A`, and check what is staged before committing.
- If a task needs a local config file, keep it uncommitted.

## Pull requests

- Every pull request must include screenshots of what it changes, taken from the running app. Show the page before and after when the change alters something that already exists.
- If a pull request changes nothing visible, such as documentation or build files, say so in place of the screenshots.
- Commit screenshots to `docs/screenshots/` on the pull request's branch, and link them from its description using a raw GitHub URL with the commit's hash.
- Take screenshots with throwaway data by setting `RICOCHET_DATA` to a temporary folder, so that they never show real requests, credentials or personal details.
