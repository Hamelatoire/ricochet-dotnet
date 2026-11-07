using System.Net;
using Ricochet.Core;

namespace Ricochet.Core.Tests;

public sealed class FolderTagTests : IDisposable
{
    private readonly string _root = Directory.CreateTempSubdirectory("ricochet-tags").FullName;

    public void Dispose() => Directory.Delete(_root, recursive: true);

    [Fact]
    public void Normalize_trims_drops_blanks_and_repeats_ignoring_case()
    {
        Assert.Equal(["smoke", "slow"], FolderTags.Normalize([" smoke ", "", "SMOKE", "slow"]));
        Assert.Null(FolderTags.Normalize([" ", ""]));
        Assert.Null(FolderTags.Normalize(null));
    }

    [Fact]
    public void Parse_splits_typed_text_on_commas()
    {
        Assert.Equal(["smoke", "slow"], FolderTags.Parse(" smoke, slow ,,"));
        Assert.Empty(FolderTags.Parse("  "));
    }

    [Fact]
    public void Selects_with_include_needs_one_of_the_tags_and_exclude_wins()
    {
        Assert.True(FolderTags.Selects(["Smoke"], ["smoke"], []));
        Assert.False(FolderTags.Selects(["api"], ["smoke"], []));
        Assert.True(FolderTags.Selects([], [], []));
        Assert.False(FolderTags.Selects(["smoke", "slow"], ["smoke"], ["SLOW"]));
    }

    [Fact]
    public void Tags_round_trip_through_the_folder_file_and_untagged_folders_add_nothing()
    {
        var workspace = Workspace(
            new CollectionItem { Id = "f1aaaaaaaa", Name = "Smoke", Tags = ["smoke", "fast"], Children = [Saved("r1aaaaaaaa", "One", "http://x.test/1")] },
            new CollectionItem { Id = "f2aaaaaaaa", Name = "Plain", Children = [Saved("r2aaaaaaaa", "Two", "http://x.test/2")] });
        var folder = new WorkspaceFolder(_root);
        folder.Load();
        folder.Save(workspace);

        var (loaded, issues) = new WorkspaceFolder(_root).Load();

        Assert.Empty(issues);
        var items = loaded.Collections[0].Items;
        Assert.Equal(["smoke", "fast"], items.Single(i => i.Name == "Smoke").Tags);
        Assert.Null(items.Single(i => i.Name == "Plain").Tags);
        var plainFolder = Directory.EnumerateFiles(_root, "folder.json", SearchOption.AllDirectories)
            .Single(f => File.ReadAllText(f).Contains("\"Plain\""));
        Assert.DoesNotContain("tags", File.ReadAllText(plainFolder));
    }

    [Fact]
    public async Task Requests_inherit_the_tags_of_every_folder_above_them()
    {
        var (runner, server) = Runner();
        var workspace = Workspace(
            new CollectionItem { Name = "Smoke", Tags = ["smoke"], Children = [Item("1", "http://x.test/1"), new CollectionItem { Name = "Deep", Children = [Item("2", "http://x.test/2")] }] },
            Item("3", "http://x.test/3"));

        await runner.RunAsync(workspace, new RunOptions { IncludeTags = ["SMOKE"] }, cancellationToken: TestContext.Current.CancellationToken);

        Assert.Equal(["/1", "/2"], server.Received.Select(r => r.RequestUri!.AbsolutePath));
    }

    [Fact]
    public async Task A_tag_on_one_folder_does_not_reach_its_siblings()
    {
        var (runner, server) = Runner();
        var workspace = Workspace(
            new CollectionItem { Name = "A", Tags = ["slow"], Children = [Item("1", "http://x.test/1")] },
            new CollectionItem { Name = "B", Children = [Item("2", "http://x.test/2")] });

        await runner.RunAsync(workspace, new RunOptions { ExcludeTags = ["slow"] }, cancellationToken: TestContext.Current.CancellationToken);

        Assert.Equal(["/2"], server.Received.Select(r => r.RequestUri!.AbsolutePath));
    }

    [Fact]
    public async Task Exclude_wins_over_include()
    {
        var (runner, server) = Runner();
        var workspace = Workspace(
            new CollectionItem { Name = "A", Tags = ["smoke", "slow"], Children = [Item("1", "http://x.test/1")] },
            new CollectionItem { Name = "B", Tags = ["smoke"], Children = [Item("2", "http://x.test/2")] });

        await runner.RunAsync(workspace, new RunOptions { IncludeTags = ["smoke"], ExcludeTags = ["slow"] }, cancellationToken: TestContext.Current.CancellationToken);

        Assert.Equal(["/2"], server.Received.Select(r => r.RequestUri!.AbsolutePath));
    }

    [Fact]
    public async Task A_tag_filter_that_selects_nothing_is_an_error_not_a_pass()
    {
        var (runner, _) = Runner();
        var workspace = Workspace(new CollectionItem { Name = "A", Tags = ["api"], Children = [Item("1", "http://x.test/1")] });

        var ex = await Assert.ThrowsAsync<RunSetupException>(() =>
            runner.RunAsync(workspace, new RunOptions { IncludeTags = ["smoke"] }, cancellationToken: TestContext.Current.CancellationToken));
        Assert.Contains("No requests match", ex.Message);
    }

    private static Workspace Workspace(params CollectionItem[] items) => new() { Collections = [new Collection { Name = "Shop", Items = items }] };

    private static CollectionItem Item(string name, string url) => new() { Name = name, Request = new ApiRequest { Name = name, Url = url } };

    private static CollectionItem Saved(string id, string name, string url) => new() { Id = id, Name = name, Request = new ApiRequest { Name = name, Url = url } };

    private static (CollectionRunner Runner, Recorder Server) Runner()
    {
        var server = new Recorder();
        return (new CollectionRunner(new RequestSender(new HttpClient(server))), server);
    }

    private sealed class Recorder : HttpMessageHandler
    {
        public List<HttpRequestMessage> Received { get; } = [];

        protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken)
        {
            Received.Add(request);
            return Task.FromResult(new HttpResponseMessage(HttpStatusCode.OK) { Content = new StringContent("{}") });
        }
    }
}
