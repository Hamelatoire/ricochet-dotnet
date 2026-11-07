namespace Ricochet.App.ViewModels;

/// <summary>Where the keyboard focus should go when the view is asked to move it.</summary>
public enum PaletteFocus
{
    PaletteQuery,
    Url,
    Environment,
}

/// <summary>One line in the command palette: an action, or a Request to open.</summary>
public sealed class PaletteEntryViewModel(string name, string detail, Action run)
{
    public string Name { get; } = name;

    /// <summary>The shortcut for an action, or the method of a Request.</summary>
    public string Detail { get; } = detail;

    public void Run() => run();
}
