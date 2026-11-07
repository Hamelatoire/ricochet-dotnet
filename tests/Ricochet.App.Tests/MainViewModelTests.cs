using Ricochet.App.ViewModels;
using static Ricochet.App.Tests.WorkspaceFixtures;

namespace Ricochet.App.Tests;

/// <summary>
/// Tests the main window's logic with no window. Each test works in its own temporary Workspace,
/// and reopens it through a new store to check what was saved.
/// </summary>
public sealed class MainViewModelTests : IDisposable
{
    private readonly TempWorkspace _workspace = new();
    private readonly StubServer _server = new();

    public MainViewModelTests() => Seed(_workspace, _server.Url);

    public void Dispose()
    {
        _server.Dispose();
        _workspace.Dispose();
    }

    private MainViewModel Open() => WorkspaceFixtures.Open(_workspace);

    [Fact]
    public void Hiding_a_folder_is_kept_after_the_Workspace_is_reopened()
    {
        var vm = Open();

        Node(vm, "Shop API", "Orders").HideOrShowCommand.Execute(null);
        vm.SaveBeforeExit();

        var reopened = Open();
        var orders = Node(reopened, "Shop API", "Orders");
        Assert.True(orders.IsHidden);
        Assert.DoesNotContain(orders, reopened.Collections.Single().VisibleChildren);
        Assert.Equal(1, reopened.Collections.Single().HiddenCount);
    }

    [Fact]
    public void Showing_hidden_items_from_the_collection_is_kept_after_the_Workspace_is_reopened()
    {
        var vm = Open();
        Node(vm, "Shop API", "Orders").HideOrShowCommand.Execute(null);
        vm.SaveBeforeExit();

        var shown = Open();
        shown.Collections.Single().HideOrShowCommand.Execute(null);
        shown.SaveBeforeExit();

        var reopened = Open();
        Assert.False(Node(reopened, "Shop API", "Orders").IsHidden);
        Assert.Equal(0, reopened.Collections.Single().HiddenCount);
    }

    [Fact]
    public void Hiding_the_selected_request_clears_the_selection()
    {
        var vm = Open();
        var create = Node(vm, "Shop API", "Orders", "Create order");
        vm.SelectedNode = create;

        create.HideOrShowCommand.Execute(null);

        Assert.Null(vm.SelectedNode);
    }

    [Fact]
    public void Arrow_keys_move_through_the_visible_rows_and_skip_hidden_ones()
    {
        var vm = Open();
        var shop = Node(vm, "Shop API");
        shop.IsExpanded = true;
        Node(vm, "Shop API", "Orders").IsExpanded = true;

        vm.MoveTreeSelectionCommand.Execute(1);
        Assert.Same(shop, vm.SelectedNode);
        vm.MoveTreeSelectionCommand.Execute(1);
        Assert.Same(Node(vm, "Shop API", "Orders"), vm.SelectedNode);
        vm.MoveTreeSelectionCommand.Execute(1);
        Assert.Same(Node(vm, "Shop API", "Orders", "Create order"), vm.SelectedNode);
        vm.MoveTreeSelectionCommand.Execute(1);
        Assert.Same(Node(vm, "Shop API", "List products"), vm.SelectedNode);
        vm.MoveTreeSelectionCommand.Execute(1);
        Assert.Same(Node(vm, "Shop API", "List products"), vm.SelectedNode);

        Node(vm, "Shop API", "Orders").HideOrShowCommand.Execute(null);
        vm.SelectedNode = shop;
        vm.MoveTreeSelectionCommand.Execute(1);
        Assert.Same(Node(vm, "Shop API", "List products"), vm.SelectedNode);
    }

    [Fact]
    public void Renaming_a_folder_is_kept_after_the_Workspace_is_reopened()
    {
        var vm = Open();
        var orders = Node(vm, "Shop API", "Orders");
        vm.SelectedNode = orders;

        vm.RenameTreeSelectionCommand.Execute(null);
        Assert.True(orders.IsRenaming);

        orders.RenameText = "Billing";
        orders.CommitRenameCommand.Execute(null);
        vm.SaveBeforeExit();

        var reopened = Open();
        Assert.Equal("Billing", Node(reopened, "Shop API", "Billing").Name);
    }

    [Fact]
    public void New_folder_is_added_selected_and_open_for_its_name_then_kept()
    {
        var vm = Open();
        var shop = Node(vm, "Shop API");

        shop.NewFolderCommand.Execute(null);
        shop.NewFolderCommand.Execute(null);

        var first = shop.Children.Single(c => c.Name == "New folder");
        var second = shop.Children.Single(c => c.Name == "New folder (2)");
        Assert.Same(second, vm.SelectedNode);
        Assert.True(second.IsRenaming);
        Assert.True(shop.IsExpanded);
        Assert.True(first.IsContainer);

        vm.SaveBeforeExit();
        var reopened = Open();
        Assert.Contains(reopened.Collections.Single().Children, c => c.Name == "New folder" && c.IsContainer);
        Assert.Contains(reopened.Collections.Single().Children, c => c.Name == "New folder (2)");
    }

    [Fact]
    public async Task Sending_a_request_goes_through_the_sender_and_shows_the_response()
    {
        var vm = Open();
        vm.SelectedNode = Node(vm, "Shop API", "Orders", "Create order");

        await vm.SendOrCancelCommand.ExecuteAsync(null);

        Assert.Equal("POST /orders HTTP/1.1", Assert.Single(_server.RequestLines));
        Assert.Equal("201 Created", vm.StatusText);
        Assert.Contains("\"ok\"", vm.ResponseBody);
        Assert.False(vm.IsSending);
        Assert.Single(vm.History);
        Assert.Equal(201, vm.History[0].StatusCode);
    }
}
