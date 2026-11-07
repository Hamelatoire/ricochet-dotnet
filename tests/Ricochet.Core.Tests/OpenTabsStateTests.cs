using Ricochet.Core;

namespace Ricochet.Core.Tests;

public sealed class OpenTabsStateTests : IDisposable
{
    private readonly string _dir = Directory.CreateTempSubdirectory("ricochet-tabs").FullName;

    public void Dispose() => Directory.Delete(_dir, recursive: true);

    [Fact]
    public void Open_tabs_and_the_active_tab_survive_a_restart()
    {
        new WorkspaceStore(_dir).SetOpenTabs(["req-a", "req-b"], "req-b");

        var reopened = new WorkspaceStore(_dir);
        reopened.Load();

        Assert.Equal(["req-a", "req-b"], reopened.OpenTabIds);
        Assert.Equal("req-b", reopened.ActiveTabId);
    }

    [Fact]
    public void With_no_tabs_open_nothing_is_written_for_them()
    {
        var store = new WorkspaceStore(_dir);
        store.SetOpenTabs([], "req-a");

        var reopened = new WorkspaceStore(_dir);
        reopened.Load();

        Assert.Empty(reopened.OpenTabIds);
        Assert.Null(reopened.ActiveTabId);
    }

    [Fact]
    public void A_state_file_from_before_tabs_opens_with_none()
    {
        new WorkspaceStore(_dir).SaveSettings(new PersonalSettings());

        var reopened = new WorkspaceStore(_dir);
        reopened.Load();

        Assert.Empty(reopened.OpenTabIds);
    }
}
