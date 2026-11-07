using System.Net;
using Ricochet.Core;

namespace Ricochet.Core.Tests;

/// <summary>Folder Variables sit between the Collection's and the Environment's (ADR 0020).</summary>
public sealed class FolderVariablesTests : IDisposable
{
    private readonly string _root = Directory.CreateTempSubdirectory("ricochet-folder-vars").FullName;

    public void Dispose() => Directory.Delete(_root, recursive: true);

    private sealed class UriHandler : HttpMessageHandler
    {
        public List<Uri> Sent { get; } = [];

        protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken)
        {
            Sent.Add(request.RequestUri!);
            return Task.FromResult(new HttpResponseMessage(HttpStatusCode.OK));
        }
    }

    // Every test request uses the same URL, so the value a request resolves to is its whole path.
    private static ApiRequest Request(string id) => new() { Id = "r" + id, Name = id, Url = "http://x.test/{{name}}" };

    private static CollectionItem Leaf(string id) => new() { Id = id, Name = id, Request = Request(id) };

    private static CollectionItem Folder(string id, IReadOnlyList<KeyValue> variables, params CollectionItem[] children) =>
        new() { Id = id, Name = id, Variables = variables, Children = children };

    private static Collection Shop(params CollectionItem[] items) => new()
    {
        Id = "c1",
        Name = "Shop",
        Variables = [new("name", "collection")],
        Items = items,
    };

    // The value {{name}} resolves to for a request, layered the way a send layers it: Collection, Folders, Environment, then overrides.
    private static string NameFor(Collection collection, string itemId, IEnumerable<KeyValue>? environment = null, params KeyValue[] overrides)
    {
        var variables = VariableResolver.Layer(collection.Variables, VariableResolver.FolderScope(collection, itemId), environment, overrides);
        return VariableResolver.Resolve(Request(itemId), variables).Sent.Url;
    }

    [Fact]
    public void A_folder_variable_overrides_the_collection_variable_for_its_requests()
    {
        var collection = Shop(Folder("f1", [new("name", "folder")], Leaf("a")), Leaf("b"));

        Assert.Equal("http://x.test/folder", NameFor(collection, "a"));
        Assert.Equal("http://x.test/collection", NameFor(collection, "b"));
    }

    [Fact]
    public void An_environment_value_overrides_the_folder_variable()
    {
        var collection = Shop(Folder("f1", [new("name", "folder")], Leaf("a")));

        Assert.Equal("http://x.test/env", NameFor(collection, "a", [new("name", "env")]));
    }

    [Fact]
    public void An_override_outranks_the_environment_and_the_folder_variable()
    {
        var collection = Shop(Folder("f1", [new("name", "folder")], Leaf("a")));

        Assert.Equal("http://x.test/override", NameFor(collection, "a", [new("name", "env")], new KeyValue("name", "override")));
    }

    [Fact]
    public void A_folder_nearer_the_request_overrides_one_further_out()
    {
        var collection = Shop(Folder("outer", [new("name", "outer")],
            Leaf("a"),
            Folder("inner", [new("name", "inner")], Leaf("b"))));

        Assert.Equal("http://x.test/outer", NameFor(collection, "a"));
        Assert.Equal("http://x.test/inner", NameFor(collection, "b"));
    }

    [Fact]
    public void A_disabled_folder_variable_does_not_hide_the_collection_value()
    {
        var collection = Shop(Folder("f1", [new("name", "folder", Enabled: false)], Leaf("a")));

        Assert.Equal("http://x.test/collection", NameFor(collection, "a"));
    }

    [Fact]
    public void A_folder_does_not_apply_to_requests_outside_it()
    {
        var collection = Shop(Folder("f1", [new("name", "folder")], Leaf("a")), Folder("f2", [], Leaf("b")));

        Assert.Equal("http://x.test/collection", NameFor(collection, "b"));
    }

    [Fact]
    public void A_request_directly_in_the_collection_has_no_folder_scope()
    {
        var collection = Shop(Folder("f1", [new("name", "folder")], Leaf("a")), Leaf("b"));

        Assert.Empty(VariableResolver.FolderScope(collection, "b"));
        Assert.Empty(VariableResolver.FolderScope(collection, null));
        Assert.Empty(VariableResolver.FolderScope(collection, "missing"));
    }

    [Fact]
    public async Task A_run_sends_each_request_with_the_folder_variables_of_its_own_folder()
    {
        var workspace = new Workspace
        {
            Collections = [Shop(Folder("f1", [new("name", "folder")], Leaf("a")), Leaf("b"))],
        };
        var handler = new UriHandler();

        var report = await new CollectionRunner(new RequestSender(new HttpClient(handler)))
            .RunAsync(workspace, new RunOptions(), cancellationToken: TestContext.Current.CancellationToken);

        Assert.Equal(2, report.Results.Count);
        Assert.Equal(["http://x.test/folder", "http://x.test/collection"], handler.Sent.Select(u => u.ToString()));
    }

    [Fact]
    public void Folder_variables_are_saved_and_loaded_and_a_folder_without_them_is_unchanged()
    {
        var workspace = new Workspace
        {
            Collections = [Shop(Folder("f1", [new("base", "http://folder.test")], Leaf("a")), Folder("f2", [], Leaf("b")))],
        };
        var folder = new WorkspaceFolder(_root);
        folder.Load();
        folder.Save(workspace);

        var (loaded, issues) = new WorkspaceFolder(_root).Load();

        Assert.Empty(issues);
        var items = loaded.Collections.Single().Items;
        Assert.Equal([new KeyValue("base", "http://folder.test")], items[0].Variables);
        Assert.Empty(items[1].Variables);

        var folderFiles = Directory.EnumerateFiles(_root, "folder.json", SearchOption.AllDirectories)
            .Select(File.ReadAllText).OrderBy(text => text.Contains("\"variables\"") ? 0 : 1).ToList();
        Assert.Contains("\"variables\"", folderFiles[0]);
        Assert.DoesNotContain("\"variables\"", folderFiles[1]);
    }

    [Fact]
    public void A_secret_folder_variable_stays_out_of_the_folder_file_and_comes_back_on_load()
    {
        var workspace = new Workspace
        {
            Collections = [Shop(Folder("f1", [new("token", "hunter2", IsSecret: true)], Leaf("a")))],
        };

        var (clean, secrets) = WorkspaceSecrets.Split(workspace);
        var folder = new WorkspaceFolder(_root);
        folder.Load();
        folder.Save(clean);

        var text = string.Concat(Directory.EnumerateFiles(_root, "*.json", SearchOption.AllDirectories).Select(File.ReadAllText));
        Assert.DoesNotContain("hunter2", text);

        var (loaded, _) = new WorkspaceFolder(_root).Load();
        var restored = WorkspaceSecrets.Merge(loaded, secrets);
        Assert.Equal("hunter2", restored.Collections.Single().Items.Single().Variables.Single().Value);
    }
}
