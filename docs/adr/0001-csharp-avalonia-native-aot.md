---
status: accepted (built)
---

# C#, Avalonia and Native AOT for the desktop app

The app must be fast and run on Windows, macOS and Linux, and its owner reads C# but not Rust. We chose C# on .NET with Avalonia for the UI, published with Native AOT, because keeping the code readable and debuggable by its owner matters more than the small runtime gain Rust could offer. Most of the sluggishness in tools like Postman comes from Electron, which this stack avoids.

## Considered Options

- **Rust (Tauri or egui):** slightly lighter at runtime, but its owner could not review or debug AI-written code. Tauri also brings back a web UI.
- **.NET MAUI:** has no official Linux support.
- **Electron:** the performance problem this project exists to avoid.

## Consequences

- All code must be AOT-safe: compiled XAML bindings, source-generated JSON, and no runtime reflection or dynamic code loading.
- Native AOT cannot cross-compile between operating systems, so each platform's release must be built on that platform.
