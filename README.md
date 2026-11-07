# Ricochet

<img src="src/Ricochet.App/Assets/ricochet-256.png" alt="The Ricochet logo" width="96" align="right">

A fast, native desktop HTTP client for Windows, macOS and Linux, built with C#, Avalonia and Native AOT.

- Decisions: [docs/adr](docs/adr)
- Vocabulary: [GLOSSARY.md](GLOSSARY.md)

## Status

Working today: sending requests with headers, bodies and auth (bearer, basic, API key), collections with folders, environments, scoped `{{variables}}`, secrets in OS secure storage, tests with a CI command, history, and formatted JSON responses.

## Install on Windows

Download `ricochet-setup-<version>-win-x64.exe` from the [latest release](https://github.com/Hamelatoire/ricochet-dotnet/releases/latest) and run it. The installer puts Ricochet in your own programs folder, so it needs no administrator rights. It can also add a desktop shortcut and put the `ricochet` command on your `PATH`, and both are off until you choose them. Uninstalling removes the program and leaves your workspace and settings alone.

| Install mode | License | Finished |
| --- | --- | --- |
| ![The install mode dialog](docs/screenshots/installer-mode.png) | ![The license page](docs/screenshots/installer-license.png) | ![The final page](docs/screenshots/installer-finish.png) |

Release builds aren't signed yet, so Windows may warn before running the installer. The zips for each platform on the same page need no installer.

## Themes

Ricochet follows your system's light or dark setting. Choose **System**, **Light** or **Dark** in the Theme box at the bottom of the sidebar. See [ADR 0013](docs/adr/0013-light-and-dark-theme.md).

| Light | Dark |
| --- | --- |
| ![Ricochet in the light theme](docs/screenshots/theme-light.png) | ![Ricochet in the dark theme](docs/screenshots/theme-dark.png) |

## Settings

Choose **Settings** at the bottom of the sidebar for preferences that stay on this machine: request timeout, following redirects, certificate checks, the largest response to keep, and wrapping long lines. They are never saved in the workspace, so a `git pull` cannot change them. While certificate checks are off, a warning shows on every page. See [ADR 0007](docs/adr/0007-personal-settings-page.md).

![The Settings page](docs/screenshots/settings-page-light.png)

## Git branch

When the workspace folder is a git repository, the sidebar shows its branch under the folder name, with commits ahead and behind its upstream. Ricochet only reads this, using the git on your computer. It never fetches, commits or pushes, so the counts reflect what git last fetched. See [ADR 0011](docs/adr/0011-read-only-git-status.md).

![The sidebar showing the git branch](docs/screenshots/sidebar-git-branch.png)

## Collections from an OpenAPI URL

Click **URL** in the sidebar, paste the address of an OpenAPI 3 description in JSON, and press **Fetch**. Ricochet shows what it will add or change before you press **Apply**. Each operation becomes a request, grouped into folders by tag. Running the same address again updates that collection and keeps the headers, bodies, tests and saved values you added. See [ADR 0014](docs/adr/0014-collections-from-openapi-url.md).

| Preview before applying | Stripe's public API, imported (612 requests) |
| --- | --- |
| ![Preview of an OpenAPI import](docs/screenshots/openapi-preview-light.png) | ![Stripe API imported into Ricochet](docs/screenshots/openapi-stripe-nested-dark.png) |

## Import from Postman

Export your collections (v2.0 or v2.1), environments and globals from Postman as JSON, then choose **Import** and select the files. The results list everything that could not be carried over. Scripts are kept but not run, and OAuth 2 and file uploads are not imported yet. See [ADR 0002](docs/adr/0002-import-postman-exports.md) for details.

## Test your APIs

Add checks to a request in its **Tests** tab, such as "Status equals 200" or "JSON value `items[0].id` exists". They run every time you send it, and the results appear beside the response. Save the request to keep them. See [ADR 0006](docs/adr/0006-declarative-tests-not-scripts.md) for why these are checks rather than scripts.

Paths for JSON values look like `items[0].name` or `$.a['b.c']`, and `items.length` counts an array. Expected values can use `{{variables}}`.

To chain requests, add a saved value in the same tab. For example, on a login request, save the JSON value `data.token` as `token`, then use `{{token}}` in later requests in that collection. Saved values stay in memory and are never written to a file. In the app they last until you close it, so send the login again after a restart, and you can see their names or clear them under **Variables > Saved**.

## Run in CI

The `ricochet` command runs a workspace's requests and checks, and only reads the workspace. Build it with:

```bash
dotnet publish src/Ricochet.Cli -c Release -r win-x64 -o publish/cli-win-x64
```

Then run it from a workspace folder, or give it the path:

```bash
ricochet run --env Staging --xunit results.xml
```

It exits with `0` if everything passed, `1` if a request or check failed, and `2` if the run couldn't happen. These count as `2` rather than passing, because a green result would be misleading: unreadable files such as ones with merge conflicts, and selections that match nothing.

A request needing a variable with no value fails without being sent. Secrets aren't in the repository, so provide them through environment variables named `RICOCHET_VAR_<name>`, for example `RICOCHET_VAR_token`. These and saved values are masked in the output and the report, but only when the value is at least 4 characters long.

Requests run in order, and saved values carry forward within a collection. Running only one folder won't include a login request outside it, so its requests fail without being sent. Postman scripts aren't run, so the summary says how many requests had any.

On Windows, Smart App Control can block a newly built `ricochet.exe` because it's unsigned. Signing release builds is the lasting fix. In development, you can run the same code with `dotnet run --project src/Ricochet.Cli -- run`.

## Develop

Requires the .NET 10 SDK.

```bash
dotnet run --project src/Ricochet.App
```

```bash
dotnet test --project tests/Ricochet.Core.Tests
```

## Publish a native build

Run this on the operating system you are building for, because Native AOT does not cross-compile between them. Replace `win-x64` with `osx-arm64` or `linux-x64` as needed.

```bash
dotnet publish src/Ricochet.App -c Release -r win-x64 -o publish/win-x64
```

Native AOT needs a platform linker: the Visual Studio C++ build tools on Windows, Xcode command line tools on macOS, and clang on Linux.

On Windows, if linking fails with `'vswhere.exe' is not recognized`, add `C:\Program Files (x86)\Microsoft Visual Studio\Installer` to your `PATH`.

## Release

Merging a pull request into `main` runs the tests and builds the app and the CLI on Windows, Linux and macOS, then attaches them to a GitHub release with the next patch version, such as `v0.1.1`. The Windows build also gets an installer, made from `installer/ricochet.iss` with [Inno Setup](https://jrsoftware.org/isinfo.php). The pull request check builds it too, so a change that breaks it is found before it merges.

To choose the version yourself, such as a new minor version, push a tag instead:

```bash
git tag v0.2.0
```

```bash
git push origin v0.2.0
```

The builds are not code signed, so Windows and macOS may warn about them or block them.

## Your data

Your work is stored as a folder of small JSON files, one per request, so it can be shared through git. By default that folder is in your user application data (`%APPDATA%\Ricochet\workspace` on Windows). To keep it in a repository, choose **Change…** at the bottom of the sidebar and pick a folder: a folder without a workspace gets a copy after you confirm, and one that has a workspace is opened. If files change while Ricochet is open, for example after a `git pull`, it reloads them. Files it can't read, such as one with merge conflict markers, are reported and left untouched.

History, the active environment and secrets stay in your personal application data and are never written to the shared folder. Set `RICOCHET_DATA` to use a different personal folder.

Values you mark as **Secret**, and values typed into auth fields, are kept out of that file and in your system's secure storage instead: DPAPI on Windows, Keychain on macOS, and Secret Service on Linux (which needs `secret-tool` from `libsecret-tools`). The macOS and Linux versions have not been tested yet. If secure storage isn't available, Secrets last only until you close the app, and Ricochet tells you so.

**Everything else in the workspace folder is plain text, and is shared if you commit it.** That includes other variables and any credentials typed directly into a header or URL, though Ricochet warns you when it spots one. Use a `{{variable}}` marked Secret for these. History hides common credential headers.
