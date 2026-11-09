using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using Ricochet.Core;

namespace Ricochet.App.ViewModels;

/// <summary>One heading and its shortcuts on the cheat sheet (ADR 0067).</summary>
public sealed record ShortcutGroup(string Title, IReadOnlyList<ShortcutEntry> Entries);

/// <summary>
/// The shortcut cheat sheet (ADR 0067): an overlay listing every shortcut by area, opened with F1 or Ctrl+/. Its rows
/// come from <see cref="ShortcutSheet"/> in Core, so the sheet follows the same table the shortcuts are listed in.
/// </summary>
public sealed partial class MainViewModel
{
    /// <summary>The cheat sheet's groups, built once from the Core table for this operating system.</summary>
    public IReadOnlyList<ShortcutGroup> ShortcutGroups { get; } =
        ShortcutSheet.Groups(OperatingSystem.IsMacOS())
            .Select(group => new ShortcutGroup(ShortcutSheet.Title(group.Key), group.ToList()))
            .ToList();

    [ObservableProperty] private bool _isShortcutSheetOpen;

    [RelayCommand]
    private void OpenShortcutSheet() => IsShortcutSheetOpen = true;

    [RelayCommand]
    private void CloseShortcutSheet() => IsShortcutSheetOpen = false;
}
