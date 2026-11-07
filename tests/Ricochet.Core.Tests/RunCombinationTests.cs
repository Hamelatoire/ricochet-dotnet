using System.Net;
using Ricochet.Core;

namespace Ricochet.Core.Tests;

/// <summary>
/// Features the run path combines: layered Variables, hidden Folders with Folder tags (ADR 0030, 0032), and Secrets in results.
/// Folder-level Variables (ADR 0020) are not in the model yet, so they have no tests here.
/// </summary>
public class RunCombinationTests
{
    private sealed class Server(Func<HttpRequestMessage, HttpResponseMessage> respond) : HttpMessageHandler
    {
        public List<HttpRequestMessage> Received { get; } = [];

        protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken)
        {
            Received.Add(request);
            return Task.FromResult(respond(request));
        }
    }

    private static (CollectionRunner Runner, Server Server) Runner(Func<HttpRequestMessage, HttpResponseMessage>? respond = null)
    {
        var server = new Server(respond ?? (_ => new HttpResponseMessage(HttpStatusCode.OK) { Content = new StringContent("{}") }));
        return (new CollectionRunner(new RequestSender(new HttpClient(server))), server);
    }

    private static Task<RunReport> Run(CollectionRunner runner, Workspace workspace, RunOptions? options = null) =>
        runner.RunAsync(workspace, options ?? new RunOptions(), cancellationToken: TestContext.Current.CancellationToken);

    private static CollectionItem Item(string name, string url) =>
        new() { Id = name, Name = name, Request = new ApiRequest { Name = name, Url = url } };

    private static CollectionItem Folder(string id, IReadOnlyList<string>? tags, params CollectionItem[] children) =>
        new() { Id = id, Name = id, Tags = tags, Children = children };

    private static List<string> Paths(Server server) => server.Received.Select(r => r.RequestUri!.AbsolutePath).ToList();

    private static List<string> Hosts(Server server) => server.Received.Select(r => r.RequestUri!.Host).ToList();

    [Fact]
    public async Task A_collection_variable_overrides_a_global_only_for_requests_in_its_own_collection()
    {
        var (runner, server) = Runner();
        var workspace = new Workspace
        {
            Globals = [new KeyValue("host", "global.test")],
            Collections =
            [
                new Collection { Name = "Shop", Variables = [new KeyValue("host", "shop.test")], Items = [Item("A", "http://{{host}}/a")] },
                new Collection { Name = "Other", Items = [Item("B", "http://{{host}}/b")] },
            ],
        };

        var report = await Run(runner, workspace);

        Assert.True(report.Passed);
        Assert.Equal(["shop.test", "global.test"], Hosts(server));
    }

    [Fact]
    public async Task An_environment_overrides_a_collection_variable_and_an_env_file_value_overrides_both()
    {
        var (runner, server) = Runner();
        var workspace = new Workspace
        {
            Environments = [new ApiEnvironment { Name = "Staging", Variables = [new KeyValue("host", "env.test")] }],
            Collections =
            [
                new Collection { Name = "Shop", Variables = [new KeyValue("host", "collection.test")], Items = [Item("A", "http://{{host}}/a")] },
            ],
        };

        await Run(runner, workspace, new RunOptions { EnvironmentName = "Staging" });
        // The CLI passes .env values as Overrides (ADR 0021), which win over every other scope.
        await Run(runner, workspace, new RunOptions { EnvironmentName = "Staging", Overrides = [new KeyValue("host", "dotenv.test")] });

        Assert.Equal(["env.test", "dotenv.test"], Hosts(server));
    }

    [Fact]
    public async Task A_secret_collection_variable_stays_masked_in_a_failed_assertion_when_a_global_shares_its_name()
    {
        // The server echoes the Secret and a non-secret global's text, so the failure message carries both.
        var (runner, _) = Runner(_ => new HttpResponseMessage(HttpStatusCode.OK) { Content = new StringContent("plain-global collection-secret-value") });
        var workspace = new Workspace
        {
            Globals = [new KeyValue("token", "plain-global")],
            Collections =
            [
                new Collection
                {
                    Name = "Shop",
                    Variables = [new KeyValue("token", "collection-secret-value", IsSecret: true)],
                    Items = [new CollectionItem
                    {
                        Id = "A",
                        Name = "A",
                        Request = new ApiRequest
                        {
                            Name = "A",
                            Url = "http://x.test/",
                            Assertions = [new RequestAssertion { Source = AssertionSources.Body, Expected = "nope" }],
                        },
                    }],
                },
            ],
        };

        var report = await Run(runner, workspace);

        var assertion = Assert.Single(report.Results[0].Assertions);
        Assert.False(assertion.Passed);
        Assert.DoesNotContain("collection-secret-value", assertion.Description);
        Assert.DoesNotContain("collection-secret-value", assertion.Message);
        Assert.Contains("***", assertion.Message);
        // The global is not a Secret, so its text is still shown.
        Assert.Contains("plain-global", assertion.Message);
    }

    [Fact]
    public async Task A_hidden_folder_with_a_matching_tag_is_skipped_and_named_while_visible_matches_still_run()
    {
        var (runner, server) = Runner();
        var workspace = new Workspace
        {
            Collections =
            [
                new Collection
                {
                    Name = "Shop",
                    HiddenItemIds = ["Hidden"],
                    Items =
                    [
                        Folder("Hidden", ["smoke"], Item("Inside", "http://x.test/inside")),
                        Folder("Open", ["smoke"], Item("Visible", "http://x.test/visible")),
                        Item("Untagged", "http://x.test/untagged"),
                    ],
                },
            ],
        };

        var report = await Run(runner, workspace, new RunOptions { IncludeTags = ["SMOKE"] });

        Assert.Equal(["/visible"], Paths(server));
        Assert.Equal("Hidden / Inside", Assert.Single(report.SkippedHidden).Path);
        Assert.Contains("1 request skipped because hidden.", RunReports.Summary(report));
        Assert.Contains(RunReports.Cautions(report), c => c.Contains("Shop / Hidden / Inside"));
    }

    [Fact]
    public async Task A_hidden_request_that_the_tag_filter_leaves_out_is_not_reported_as_skipped()
    {
        var (runner, server) = Runner();
        var workspace = new Workspace
        {
            Collections =
            [
                new Collection
                {
                    Name = "Shop",
                    HiddenItemIds = ["Hidden"],
                    Items =
                    [
                        Folder("Hidden", ["slow"], Item("Inside", "http://x.test/inside")),
                        Folder("Open", ["smoke"], Item("Visible", "http://x.test/visible")),
                    ],
                },
            ],
        };

        var report = await Run(runner, workspace, new RunOptions { IncludeTags = ["smoke"] });

        Assert.Equal(["/visible"], Paths(server));
        Assert.Empty(report.SkippedHidden);
    }

    [Fact]
    public async Task A_subfolder_inherits_its_parents_tag_and_is_reported_when_hidden()
    {
        var (runner, server) = Runner();
        var workspace = new Workspace
        {
            Collections =
            [
                new Collection
                {
                    Name = "Shop",
                    HiddenItemIds = ["Inner"],
                    Items =
                    [
                        Folder("Outer", ["smoke"],
                            Folder("Inner", null, Item("Deep", "http://x.test/deep")),
                            Item("Shallow", "http://x.test/shallow")),
                        Folder("Other", null, Item("Untagged", "http://x.test/untagged")),
                    ],
                },
            ],
        };

        var report = await Run(runner, workspace, new RunOptions { IncludeTags = ["smoke"] });

        Assert.Equal(["/shallow"], Paths(server));
        Assert.Equal("Outer / Inner / Deep", Assert.Single(report.SkippedHidden).Path);
    }

    [Fact]
    public async Task When_the_only_tag_match_is_hidden_the_run_cannot_happen_and_nothing_is_sent()
    {
        var (runner, server) = Runner();
        var workspace = new Workspace
        {
            Collections =
            [
                new Collection
                {
                    Name = "Shop",
                    HiddenItemIds = ["Hidden"],
                    Items =
                    [
                        Folder("Hidden", ["smoke"], Item("Inside", "http://x.test/inside")),
                        Folder("Other", ["api"], Item("Api", "http://x.test/api")),
                    ],
                },
            ],
        };

        var error = await Assert.ThrowsAsync<RunSetupException>(() => Run(runner, workspace, new RunOptions { IncludeTags = ["smoke"] }));

        Assert.Empty(server.Received);
        Assert.Equal("The only request that would run is hidden. Show it in the app to run it.", error.Message);
    }

    [Fact]
    public async Task A_hidden_tag_match_that_is_also_excluded_is_not_reported_and_the_run_says_no_tags_match()
    {
        var (runner, server) = Runner();
        var workspace = new Workspace
        {
            Collections =
            [
                new Collection
                {
                    Name = "Shop",
                    HiddenItemIds = ["Hidden"],
                    Items = [Folder("Hidden", ["smoke"], Item("Inside", "http://x.test/inside"))],
                },
            ],
        };

        // Exclusion is applied before hiding, so the excluded request is not named as a hidden skip.
        var error = await Assert.ThrowsAsync<RunSetupException>(() =>
            Run(runner, workspace, new RunOptions { IncludeTags = ["smoke"], ExcludeTags = ["smoke"] }));

        Assert.Empty(server.Received);
        Assert.Equal("No requests match the tags given.", error.Message);
    }
}
