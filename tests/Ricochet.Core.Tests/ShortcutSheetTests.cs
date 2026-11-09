using Ricochet.Core;

namespace Ricochet.Core.Tests;

public sealed class ShortcutSheetTests
{
    [Fact]
    public void Every_shortcut_names_an_area_keys_and_an_action()
    {
        foreach (var entry in ShortcutSheet.Entries(macOs: false))
        {
            Assert.True(Enum.IsDefined(entry.Area));
            Assert.False(string.IsNullOrWhiteSpace(entry.Keys));
            Assert.False(string.IsNullOrWhiteSpace(entry.Action));
        }
    }

    [Fact]
    public void Shortcuts_use_Ctrl_on_Windows_and_Cmd_on_macOS()
    {
        Assert.Contains(ShortcutSheet.Entries(macOs: false), e => e.Keys == "Ctrl+S");
        Assert.Contains(ShortcutSheet.Entries(macOs: true), e => e.Keys == "Cmd+S");
        Assert.DoesNotContain(ShortcutSheet.Entries(macOs: true), e => e.Keys.StartsWith("Ctrl+"));
    }

    [Fact]
    public void Groups_follow_the_area_order_and_keep_every_entry()
    {
        var groups = ShortcutSheet.Groups(macOs: false);

        Assert.Equal([ShortcutArea.General, ShortcutArea.Request, ShortcutArea.Palette], groups.Select(g => g.Key));
        Assert.Equal(ShortcutSheet.Entries(macOs: false).Count, groups.Sum(g => g.Count()));
    }

    [Fact]
    public void Each_area_has_a_heading()
    {
        foreach (var area in Enum.GetValues<ShortcutArea>())
            Assert.False(string.IsNullOrWhiteSpace(ShortcutSheet.Title(area)));
    }
}
