using Avalonia.Controls;
using Avalonia.Headless;
using Avalonia.VisualTree;
using Ricochet.App.Views;
using static Ricochet.App.Tests.WorkspaceFixtures;

namespace Ricochet.App.Tests;

/// <summary>
/// The sidebar's Collections tree, drawn by the real window with no display. Checks the bindings that decide which rows are drawn.
/// </summary>
public sealed class SidebarTreeTests : IDisposable
{
    private readonly TempWorkspace _workspace = new();

    public SidebarTreeTests() => Seed(_workspace, path => "http://127.0.0.1" + path);

    public void Dispose() => _workspace.Dispose();

    [Fact]
    public async Task Hidden_folders_and_the_requests_inside_them_are_not_drawn()
    {
        // Avalonia controls belong to the UI thread, so the window is built and read there.
        List<string> rows = [];
        await AvaloniaTestHost.Session().Dispatch(() =>
        {
            var vm = Open(_workspace);
            Node(vm, "Shop API").IsExpanded = true;
            Node(vm, "Shop API", "Orders").HideOrShowCommand.Execute(null);

            var window = new MainWindow { DataContext = vm };
            window.Show();
            window.CaptureRenderedFrame(); // Lays out and draws the window, so the tree creates its rows.
            var tree = window.FindControl<TreeView>("CollectionsTree")!;
            rows = tree.GetVisualDescendants().OfType<TextBlock>().Select(t => t.Text).ToList();
            window.Close();
        }, CancellationToken.None);

        Assert.Contains("Shop API", rows);
        Assert.Contains("List products", rows);
        Assert.DoesNotContain("Orders", rows);
        Assert.DoesNotContain("Create order", rows);
    }
}
