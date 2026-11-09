namespace Ricochet.Core;

/// <summary>The part of the app where a shortcut works, used to group the cheat sheet (ADR 0067).</summary>
public enum ShortcutArea
{
    General,
    Request,
    Palette,
}

/// <summary>One shortcut as the cheat sheet shows it: the keys, what they do, and where they work.</summary>
public sealed record ShortcutEntry(ShortcutArea Area, string Keys, string Action);

/// <summary>
/// The shortcut table behind the cheat sheet (ADR 0067). Each entry names its area, so the sheet can group it, and
/// the keys are written for the operating system: Cmd on macOS, Ctrl elsewhere (ADR 0040).
/// </summary>
public static class ShortcutSheet
{
    /// <summary>Every shortcut the app has, in the order the sheet shows them within each area.</summary>
    public static IReadOnlyList<ShortcutEntry> Entries(bool macOs)
    {
        var mod = macOs ? "Cmd" : "Ctrl";
        return
        [
            new(ShortcutArea.General, "F1", "Show this list of shortcuts"),
            new(ShortcutArea.General, $"{mod}+/", "Show this list of shortcuts"),
            new(ShortcutArea.General, $"{mod}+K", "Open the command palette"),
            new(ShortcutArea.General, $"{mod}+P", "Find a request"),
            new(ShortcutArea.General, $"{mod}+N", "New request"),
            new(ShortcutArea.General, $"{mod}+S", "Save"),
            new(ShortcutArea.General, $"{mod}+W", "Close the active tab"),
            new(ShortcutArea.Request, $"{mod}+Enter", "Send the request"),
            new(ShortcutArea.Request, $"{mod}+L", "Focus the URL"),
            new(ShortcutArea.Request, $"{mod}+E", "Switch environment"),
            new(ShortcutArea.Palette, "Up and Down", "Choose an entry"),
            new(ShortcutArea.Palette, "Enter", "Run the chosen entry"),
            new(ShortcutArea.Palette, "Escape", "Close the palette or this list"),
        ];
    }

    /// <summary>The entries grouped by area, in the order the areas are declared, with empty areas left out.</summary>
    public static IReadOnlyList<IGrouping<ShortcutArea, ShortcutEntry>> Groups(bool macOs) =>
        Entries(macOs).GroupBy(entry => entry.Area).OrderBy(group => group.Key).ToList();

    /// <summary>The heading shown above each area.</summary>
    public static string Title(ShortcutArea area) => area switch
    {
        ShortcutArea.General => "Everywhere",
        ShortcutArea.Request => "In a request",
        ShortcutArea.Palette => "In the palette and this list",
        _ => area.ToString(),
    };
}
