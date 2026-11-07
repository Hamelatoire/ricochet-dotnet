using System.Net;
using Ricochet.Core;

namespace Ricochet.Core.Tests;

/// <summary>ADR 0032: hidden folders and requests are not run, and the run report names them.</summary>
public class HiddenRunTests
{
    private sealed class Server(Func<HttpRequestMessage, HttpResponseMessage> respond) : HttpMessageHandler
    {
        public List<string> Paths { get; } = [];

        protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken)
        {
            Paths.Add(request.RequestUri!.AbsolutePath);
            return Task.FromResult(respond(request));
        }
    }

    private static CollectionItem Item(string name, string url) =>
        new() { Id = name, Name = name, Request = new ApiRequest { Name = name, Url = url } };

    private static CollectionItem Folder(string id, params CollectionItem[] children) => new() { Id = id, Name = id, Children = children };

    private static (CollectionRunner Runner, Server Server) Runner()
    {
        var server = new Server(_ => new HttpResponseMessage(HttpStatusCode.OK) { Content = new StringContent("ok") });
        return (new CollectionRunner(new RequestSender(new HttpClient(server))), server);
    }

    private static Task<RunReport> Run(CollectionRunner runner, Workspace workspace, RunOptions? options = null) =>
        runner.RunAsync(workspace, options ?? new RunOptions(), cancellationToken: TestContext.Current.CancellationToken);

    private static Workspace Shop(IReadOnlyList<string>? hidden, params CollectionItem[] items) =>
        new() { Collections = [new Collection { Name = "Shop", Items = items, HiddenItemIds = hidden }] };

    [Fact]
    public async Task Requests_in_a_hidden_folder_are_not_sent_and_are_named()
    {
        var (runner, server) = Runner();
        var workspace = Shop(["Hidden"],
            Item("Visible", "http://x.test/visible"),
            Folder("Hidden", Item("Inside", "http://x.test/inside")));

        var report = await Run(runner, workspace);

        Assert.Equal(["/visible"], server.Paths);
        Assert.True(report.Passed);
        var skipped = Assert.Single(report.SkippedHidden);
        Assert.Equal("Shop", skipped.Collection);
        Assert.Equal("Hidden / Inside", skipped.Path);
    }

    [Fact]
    public async Task Nested_requests_under_a_hidden_folder_are_all_skipped()
    {
        var (runner, server) = Runner();
        var workspace = Shop(["Outer"],
            Folder("Outer", Folder("Middle", Item("Deep", "http://x.test/deep"))),
            Item("Visible", "http://x.test/visible"));

        var report = await Run(runner, workspace);

        Assert.Equal(["/visible"], server.Paths);
        Assert.Equal("Outer / Middle / Deep", Assert.Single(report.SkippedHidden).Path);
    }

    [Fact]
    public async Task A_hidden_request_is_skipped_on_its_own()
    {
        var (runner, server) = Runner();
        var workspace = Shop(["Secret"], Item("Secret", "http://x.test/secret"), Item("Open", "http://x.test/open"));

        var report = await Run(runner, workspace);

        Assert.Equal(["/open"], server.Paths);
        Assert.Equal("Secret", Assert.Single(report.SkippedHidden).Path);
    }

    [Fact]
    public async Task Hiding_everything_leaves_nothing_to_run_and_the_run_cannot_happen()
    {
        var (runner, server) = Runner();
        var workspace = Shop(["Hidden"], Folder("Hidden", Item("A", "http://x.test/a"), Item("B", "http://x.test/b")));

        var error = await Assert.ThrowsAsync<RunSetupException>(() => Run(runner, workspace));

        Assert.Empty(server.Paths);
        Assert.Contains("2 requests", error.Message);
        Assert.Contains("hidden", error.Message);
    }

    [Fact]
    public async Task Hidden_requests_outside_the_selected_folder_are_not_reported_as_skipped()
    {
        var (runner, server) = Runner();
        var workspace = Shop(["Hidden"],
            Folder("Hidden", Item("A", "http://x.test/a")),
            Folder("Open", Item("B", "http://x.test/b")));

        var report = await Run(runner, workspace, new RunOptions { FolderName = "Open" });

        Assert.Equal(["/b"], server.Paths);
        Assert.Empty(report.SkippedHidden);
    }

    [Fact]
    public async Task Summary_and_cautions_say_how_many_were_skipped_and_which()
    {
        var (runner, _) = Runner();
        var workspace = Shop(["Hidden"],
            Folder("Hidden", Item("A", "http://x.test/a")),
            Item("Open", "http://x.test/open"));

        var report = await Run(runner, workspace);

        Assert.Contains("1 request skipped because hidden.", RunReports.Summary(report));
        var caution = Assert.Single(RunReports.Cautions(report), c => c.Contains("hidden"));
        Assert.Contains("Shop / Hidden / A", caution);
    }

    [Fact]
    public async Task A_run_with_no_hidden_requests_reports_none_skipped()
    {
        var (runner, _) = Runner();
        var report = await Run(runner, Shop(null, Item("A", "http://x.test/a")));

        Assert.Empty(report.SkippedHidden);
        Assert.DoesNotContain("hidden", RunReports.Summary(report));
        Assert.DoesNotContain(RunReports.Cautions(report), c => c.Contains("hidden"));
    }
}
