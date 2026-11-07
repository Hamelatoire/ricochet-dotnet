using Ricochet.Core;

namespace Ricochet.Core.Tests;

public sealed class WindowPlacementTests : IDisposable
{
    private readonly string _dir = Directory.CreateTempSubdirectory("ricochet-window").FullName;

    public void Dispose() => Directory.Delete(_dir, recursive: true);

    [Fact]
    public void Missing_window_falls_back_to_the_default()
    {
        var store = new WorkspaceStore(_dir);
        store.Load();

        Assert.Null(store.Window);
    }

    [Fact]
    public void Corrupt_state_falls_back_to_the_default_and_is_set_aside()
    {
        File.WriteAllText(Path.Combine(_dir, "state.json"), "{ this is not json");

        var store = new WorkspaceStore(_dir);
        var result = store.Load();

        Assert.Null(store.Window);
        Assert.Contains(result.Issues, issue => issue.Problem.Contains("set aside"));
        Assert.Contains(Directory.GetFiles(_dir), file => file.Contains("state.json.corrupt-"));
    }

    [Fact]
    public void Window_value_that_no_window_could_have_falls_back_to_the_default()
    {
        File.WriteAllText(Path.Combine(_dir, "state.json"), """{ "window": { "x": 10, "y": 10, "width": 0, "height": 900 } }""");

        var store = new WorkspaceStore(_dir);
        store.Load();

        Assert.Null(store.Window);
    }

    [Fact]
    public void Window_value_with_a_missing_size_falls_back_to_the_default()
    {
        File.WriteAllText(Path.Combine(_dir, "state.json"), """{ "window": { "x": 10, "y": 10 } }""");

        var store = new WorkspaceStore(_dir);
        store.Load();

        Assert.Null(store.Window);
    }

    [Fact]
    public void Saved_window_survives_a_restart_and_stays_out_of_the_workspace()
    {
        var store = new WorkspaceStore(_dir);
        store.Load();
        store.SaveWindow(new WindowPlacement { X = 1200, Y = 80, Width = 1100.5, Height = 760, IsMaximized = true });

        var reloaded = new WorkspaceStore(_dir);
        reloaded.Load();

        Assert.Equal(new WindowPlacement { X = 1200, Y = 80, Width = 1100.5, Height = 760, IsMaximized = true }, reloaded.Window);
        var sharedFiles = Directory.GetFiles(reloaded.FolderPath, "*", SearchOption.AllDirectories).Select(File.ReadAllText);
        Assert.All(sharedFiles, text => Assert.DoesNotContain("isMaximized", text));

        // Only the stored fields are written, not computed values.
        var stateText = File.ReadAllText(Path.Combine(_dir, "state.json"));
        Assert.Contains("\"isMaximized\": true", stateText);
        Assert.DoesNotContain("isReasonable", stateText, StringComparison.OrdinalIgnoreCase);
    }

    [Fact]
    public void Saving_the_window_keeps_the_other_personal_state()
    {
        var store = new WorkspaceStore(_dir);
        store.Load();
        store.SetTheme("dark");
        store.SaveWindow(new WindowPlacement { X = 5, Y = 6, Width = 1000, Height = 700 });

        var reloaded = new WorkspaceStore(_dir);
        reloaded.Load();

        Assert.Equal("dark", reloaded.Theme);
        Assert.NotNull(reloaded.Window);
    }
}
