using System.Net;
using Ricochet.Core;

namespace Ricochet.Core.Tests;

/// <summary>The .env rules where values are used and saved: they are overrides, and they never reach a Workspace file (ADR 0021).</summary>
public class DotEnvRulesTests : IDisposable
{
    private const string Secret = "local-only-token-9f3a";

    private readonly string _root = Directory.CreateTempSubdirectory("ricochet-dotenv-rules").FullName;

    public void Dispose() => Directory.Delete(_root, recursive: true);

    private sealed class Handler(Func<HttpRequestMessage, HttpResponseMessage> respond) : HttpMessageHandler
    {
        public List<HttpRequestMessage> Received { get; } = [];

        protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken)
        {
            Received.Add(request);
            return Task.FromResult(respond(request));
        }
    }

    private static Workspace Sample(params KeyValue[] collectionVariables) => new()
    {
        Collections =
        [
            new Collection
            {
                Id = "c1aaaaaaaa",
                Name = "Shop API",
                Variables = [new("base", "http://x.test"), .. collectionVariables],
                Items =
                [
                    new CollectionItem
                    {
                        Id = "i1aaaaaaaa",
                        Name = "Me",
                        Request = new ApiRequest
                        {
                            Id = "r1aaaaaaaa",
                            Name = "Me",
                            Url = "{{base}}/me",
                            Auth = new RequestAuth { Kind = AuthKinds.Bearer, Token = "{{API_TOKEN}}" },
                        },
                    },
                ],
            },
        ],
        Globals = [new("g", "1")],
    };

    [Fact]
    public async Task A_dotenv_value_is_sent_when_nothing_else_has_the_name()
    {
        var handler = new Handler(_ => new HttpResponseMessage(HttpStatusCode.OK));
        var runner = new CollectionRunner(new RequestSender(new HttpClient(handler)));

        var report = await runner.RunAsync(Sample(), new RunOptions { Overrides = [new("API_TOKEN", Secret)] },
            cancellationToken: TestContext.Current.CancellationToken);

        Assert.True(report.Passed);
        Assert.Equal($"Bearer {Secret}", handler.Received.Single().Headers.Authorization!.ToString());
    }

    [Fact]
    public async Task A_dotenv_value_overrides_a_collection_variable_but_a_command_line_value_wins_over_it()
    {
        var handler = new Handler(_ => new HttpResponseMessage(HttpStatusCode.OK));
        var runner = new CollectionRunner(new RequestSender(new HttpClient(handler)));
        var workspace = Sample(new KeyValue("API_TOKEN", "from-collection"));

        // The same order as the CLI: .env first, then --var, so the later one wins.
        await runner.RunAsync(workspace, new RunOptions { Overrides = [new("API_TOKEN", Secret), new("API_TOKEN", "from-flag")] },
            cancellationToken: TestContext.Current.CancellationToken);
        Assert.Equal("Bearer from-flag", handler.Received.Single().Headers.Authorization!.ToString());

        handler.Received.Clear();
        await runner.RunAsync(workspace, new RunOptions { Overrides = [new("API_TOKEN", Secret)] },
            cancellationToken: TestContext.Current.CancellationToken);
        Assert.Equal($"Bearer {Secret}", handler.Received.Single().Headers.Authorization!.ToString());
    }

    [Fact]
    public async Task A_bulk_run_sends_the_dotenv_value_for_each_id()
    {
        var handler = new Handler(_ => new HttpResponseMessage(HttpStatusCode.OK));
        var runner = new BulkRunner(new RequestSender(new HttpClient(handler)));
        var template = new ApiRequest
        {
            Name = "Me",
            Url = "http://x.test/{{id}}",
            Auth = new RequestAuth { Kind = AuthKinds.Bearer, Token = "{{API_TOKEN}}" },
        };

        await runner.RunAsync(new Workspace(), new Collection { Name = "Shop" }, null, template, "id", ["a1", "b2"],
            _ => { }, TestContext.Current.CancellationToken, 1, [new KeyValue("API_TOKEN", Secret)]);

        Assert.Equal(2, handler.Received.Count);
        Assert.All(handler.Received, r => Assert.Equal($"Bearer {Secret}", r.Headers.Authorization!.ToString()));
    }

    [Fact]
    public void Saving_the_workspace_writes_no_dotenv_value_and_leaves_the_file_alone()
    {
        var envPath = Path.Combine(_root, DotEnv.FileName);
        var envText = $"API_TOKEN={Secret}\n";
        File.WriteAllText(envPath, envText);

        var folder = new WorkspaceFolder(_root);
        folder.Load();
        var workspace = Sample();
        folder.Save(workspace);

        // The .env values are used by a run, which never changes the Workspace, and then the Workspace is saved again.
        var dotEnv = DotEnv.Read(_root);
        var resolved = VariableResolver.Layer(workspace.Globals, workspace.Collections[0].Variables, dotEnv.Variables);
        Assert.Equal(Secret, resolved.Single(v => v.Key == "API_TOKEN").Value);
        folder.Save(workspace);

        foreach (var path in Directory.EnumerateFiles(_root, "*", SearchOption.AllDirectories))
        {
            if (path == envPath)
                continue;
            Assert.DoesNotContain(Secret, File.ReadAllText(path));
        }

        Assert.Equal(envText, File.ReadAllText(envPath));
    }
}
