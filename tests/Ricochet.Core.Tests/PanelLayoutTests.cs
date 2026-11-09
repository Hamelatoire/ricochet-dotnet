using Ricochet.Core;

namespace Ricochet.Core.Tests;

public sealed class PanelLayoutTests : IDisposable
{
    private readonly string _dir = Directory.CreateTempSubdirectory("ricochet-panels").FullName;

    public void Dispose() => Directory.Delete(_dir, recursive: true);

    [Fact]
    public void The_sidebar_width_and_request_height_survive_a_restart()
    {
        new WorkspaceStore(_dir).SavePanels(new PanelLayout { SidebarWidth = 340, RequestHeight = 420 });

        var reopened = new WorkspaceStore(_dir);
        reopened.Load();

        Assert.Equal(340, reopened.Panels?.SidebarWidth);
        Assert.Equal(420, reopened.Panels?.RequestHeight);
    }

    [Fact]
    public void A_damaged_value_falls_back_to_its_default_and_the_other_is_kept()
    {
        new WorkspaceStore(_dir).SavePanels(new PanelLayout { SidebarWidth = -20, RequestHeight = 420 });

        var reopened = new WorkspaceStore(_dir);
        reopened.Load();

        Assert.Null(reopened.Panels?.SidebarWidth);
        Assert.Equal(420, reopened.Panels?.RequestHeight);
    }

    [Fact]
    public void A_value_that_is_not_a_number_is_dropped()
    {
        Assert.Null(new PanelLayout { SidebarWidth = double.NaN }.Sanitized().SidebarWidth);
        Assert.Null(new PanelLayout { RequestHeight = double.PositiveInfinity }.Sanitized().RequestHeight);
    }

    [Fact]
    public void A_state_file_from_before_panels_opens_with_the_defaults()
    {
        new WorkspaceStore(_dir).SaveSettings(new PersonalSettings());

        var reopened = new WorkspaceStore(_dir);
        reopened.Load();

        Assert.Null(reopened.Panels);
    }
}
