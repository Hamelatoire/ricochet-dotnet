---
status: proposed
---

# Rebuild the app in Rust with GPUI

Ricochet is a C# app on Avalonia, published with Native AOT ([ADR 0001](0001-csharp-avalonia-native-aot.md)). Discord's new desktop client is reported to be built in Rust with GPUI, the UI framework from the Zed editor. We want to test whether that stack gives a lighter, faster app than Avalonia, and we want to decide that with evidence, not by guessing. This ADR proposes a full rewrite of the app in Rust with GPUI, delivered as a stack of pull requests, and it does not replace ADR 0001 until the rewrite is accepted.

Discord has not released its client, so the claim that it uses GPUI is based on press reports and an interview with its CTO. We have not verified that GPUI is published on crates.io or that it runs well on Windows, macOS and Linux. The first pull request in the stack checks both before anything else is built.

## Decisions within this

- **Stack:** Rust for all code, with GPUI for the UI. The `Ricochet.Core`, `Ricochet.App` and `Ricochet.Cli` split stays, as Rust crates in one workspace.
- **Behaviour is the spec:** the Rust version must match the C# app's file format, Secret handling and CLI exit codes, so existing workspaces and CI pipelines keep working.
- **Staged, not big bang:** the C# app stays on `main` and keeps shipping until the Rust version reaches parity. The Rust version lives beside it until then.
- **Each pull request gets its own proof:** each step in the stack says what it changes and how it is checked. UI steps include screenshots, as CLAUDE.md requires.

## Stack of pull requests

1. This ADR, docs only.
2. Feasibility spike: a Rust workspace with GPUI, a main window, and a build for Windows, macOS and Linux, checked in CI.
3. `Ricochet.Core` port: models, the file format, variables, sending and the runner, with the existing C# tests ported as Rust tests.
4. `Ricochet.Cli` port, with the same exit codes and masking.
5. `Ricochet.App` port, in slices: sidebar, request editor, response view, then the rest.
6. Import and export: Postman, Insomnia, OpenAPI and HAR.
7. Cutover: publish the Rust build, retire the C# projects, and mark ADR 0001 as superseded.

## Considered Options

- **Stay on Avalonia:** no rewrite cost, and the current app works and is tested.
- **Rust with Tauri:** keeps a web UI, which ADR 0001 already rejected.
- **Rewrite in place, no stack:** no parallel period, so a broken rewrite would block releases.

## Consequences

- ADR 0001 says the owner reads C# but not Rust and needs to review and debug the code. A Rust rewrite reverses that trade-off, so this ADR must be accepted by the owner before any code moves.
- GPUI is pre-1.0 and is built mainly for Zed. If it is not published as a stable crate, or it does not run well on Windows, this ADR should be rejected.
- Native AOT no longer applies. The release size and startup time must be measured against the C# build, using the benchmarks from [ADR 0080](0080-benchmark-startup-and-send-overhead-in-ci.md).
- Test coverage in `tests/Ricochet.Core.Tests` and `tests/Ricochet.App.Tests` must be ported before the C# projects are retired.
- Until cutover, two codebases are maintained. Bugs fixed on `main` must be checked against the Rust branch.
