---
status: accepted
---

# Keep views thin by moving UI logic out of code-behind

`MainWindow.axaml.cs` has grown to about 400 lines. It holds the import and folder dialogs, clipboard copy, the URL key handling, and the wiring between controls and the main view model. That code can't be tested without a window, and every new dialog or shortcut adds to the same file. We will move this behaviour into view models and small services that the view calls, so the window only maps controls to those services. [ADR 0001](0001-csharp-avalonia-native-aot.md) keeps the app on Avalonia and MVVM, so this is a move within that design, not a new framework.

## Decisions within this

- **The view owns only what the platform forces:** focus, control references and the native file or folder pickers stay in code-behind, because they need a window handle. Everything else moves out.
- **Dialogs and clipboard sit behind small interfaces:** a view model asks an `IDialogs` or `IClipboard` service, and the window implements it. A test can then pass a fake and check what the user would have seen.
- **Commands replace event handlers:** each button and menu item binds to an `ICommand` on a view model, so the same action can be reached from a button, a shortcut or the palette in [ADR 0040](0040-keyboard-shortcuts-and-a-command-palette.md).
- **Move in small steps:** one handler per pull request, each with screenshots that show the screen is unchanged, so no pull request rewrites the whole window.

## Considered Options

- **Leave the code-behind as it is:** no work now, but every new feature in the window makes the next move harder and keeps its logic untestable.
- **Rewrite the window in one change:** the cleanest end state, but a large change to the most-used screen with no way to show it behaves the same.
- **Adopt a full navigation framework:** more structure than an app with one main window needs, and it adds a dependency that [ADR 0005](0005-local-first.md) gives no reason for.

## Consequences

- Most of the window's behaviour becomes testable without a window, which supports [ADR 0044](0044-test-view-models-and-headless-views.md).
- More classes to read. The trade-off is that each class has one reason to change.
- Native AOT must still see the reflection-free bindings, so compiled bindings stay on for every view moved, and the publish step is run after each move.
