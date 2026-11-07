using Ricochet.App.ViewModels;
using Ricochet.App.Services;
using Ricochet.Core;

namespace Ricochet.App.Tests;

/// <summary>The Workspace every view model test starts from: one Collection with a folder holding one request, and one request.</summary>
public static class WorkspaceFixtures
{
    public static Workspace Sample(Func<string, string> serverUrl) => new()
    {
        Collections =
        [
            new Collection
            {
                Id = "c1",
                Name = "Shop API",
                Items =
                [
                    new CollectionItem
                    {
                        Id = "f1",
                        Name = "Orders",
                        Children =
                        [
                            new CollectionItem
                            {
                                Id = "r1",
                                Name = "Create order",
                                Request = new ApiRequest { Id = "r1", Name = "Create order", Method = "POST", Url = serverUrl("/orders") },
                            },
                        ],
                    },
                    new CollectionItem
                    {
                        Id = "r2",
                        Name = "List products",
                        Request = new ApiRequest { Id = "r2", Name = "List products", Method = "GET", Url = serverUrl("/products") },
                    },
                ],
            },
        ],
    };

    /// <summary>Writes the sample Workspace into the temporary folder, as a previous run of the app would have.</summary>
    public static void Seed(TempWorkspace workspace, Func<string, string> serverUrl)
    {
        var store = workspace.NewStore();
        store.Load();
        store.Save(Sample(serverUrl));
    }

    /// <summary>Opens the Workspace as the app does at startup. Requests use the real sender, which only reaches the stub server.</summary>
    public static MainViewModel Open(TempWorkspace workspace) => new(
        new RequestSender(RequestSender.CreateDefaultClient()),
        workspace.NewStore(),
        new InMemorySecretVault(),
        new FakeDialogs(),
        new FakeClipboard());

    public static TreeNodeViewModel Node(MainViewModel vm, params string[] path)
    {
        var node = vm.Collections.Single(c => c.Name == path[0]);
        foreach (var name in path.Skip(1))
            node = node.Children.Single(c => c.Name == name);
        return node;
    }
}
