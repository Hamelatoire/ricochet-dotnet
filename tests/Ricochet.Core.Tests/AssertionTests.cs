using System.Net;
using System.Text;
using System.Text.Json;
using System.Xml.Linq;
using Ricochet.Core;

namespace Ricochet.Core.Tests;

public class AssertionEvaluatorTests
{
    private const string Json = """{ "items": [ { "id": 1, "name": "Widget", "price": 19.50, "ok": true, "none": null } ], "a.b": "dotted", "count": 2, "length": "own" }""";

    private static ApiResponse Response(string body = Json, int status = 200, int elapsedMs = 150, string mediaType = "application/json") =>
        new(status, "OK", [new("Content-Type", mediaType), new("X-Trace", "abc")], Encoding.UTF8.GetBytes(body), mediaType, null, TimeSpan.FromMilliseconds(elapsedMs), false);

    private static AssertionResult Check(string source, string path, string op, string expected = "", ApiResponse? response = null) =>
        AssertionEvaluator.Evaluate([new RequestAssertion { Source = source, Path = path, Op = op, Expected = expected }], response ?? Response()).Single();

    [Theory]
    [InlineData(AssertionOps.Equal, "200", true)]
    [InlineData(AssertionOps.Equal, "404", false)]
    [InlineData(AssertionOps.NotEqual, "404", true)]
    [InlineData(AssertionOps.LessThan, "300", true)]
    [InlineData(AssertionOps.GreaterThan, "300", false)]
    [InlineData(AssertionOps.Matches, "^2\\d\\d$", true)]
    public void Checks_status(string op, string expected, bool passed)
    {
        Assert.Equal(passed, Check(AssertionSources.Status, "", op, expected).Passed);
    }

    [Fact]
    public void Checks_time()
    {
        Assert.True(Check(AssertionSources.Time, "", AssertionOps.LessThan, "500").Passed);
        Assert.False(Check(AssertionSources.Time, "", AssertionOps.LessThan, "100").Passed);
    }

    [Fact]
    public void Checks_headers_ignoring_name_case()
    {
        Assert.True(Check(AssertionSources.Header, "content-type", AssertionOps.Contains, "json").Passed);
        Assert.True(Check(AssertionSources.Header, "X-Missing", AssertionOps.NotExists).Passed);
        Assert.False(Check(AssertionSources.Header, "X-Trace", AssertionOps.NotExists).Passed);
        Assert.Equal("not found", Check(AssertionSources.Header, "X-Missing", AssertionOps.Equal, "x").Message);
    }

    [Fact]
    public void Checks_body_text()
    {
        Assert.True(Check(AssertionSources.Body, "", AssertionOps.Contains, "Widget").Passed);
        Assert.True(Check(AssertionSources.Body, "", AssertionOps.NotContains, "Gadget").Passed);
    }

    [Theory]
    [InlineData("items[0].name", "Widget")]
    [InlineData("$.items[0].name", "Widget")]
    [InlineData("items[0]['name']", "Widget")]
    [InlineData("['a.b']", "dotted")]
    [InlineData("items[0].ok", "true")]
    [InlineData("items[0].none", "null")]
    [InlineData("items.length", "1")]
    [InlineData("length", "own")]
    public void Finds_json_values(string path, string expected)
    {
        var result = Check(AssertionSources.Json, path, AssertionOps.Equal, expected);
        Assert.True(result.Passed, result.Message);
    }

    [Theory]
    [InlineData("items[0].price", "19.5")]
    [InlineData("count", "2.0")]
    public void Numbers_compare_by_value(string path, string expected)
    {
        Assert.True(Check(AssertionSources.Json, path, AssertionOps.Equal, expected).Passed);
    }

    [Fact]
    public void Json_null_exists_but_a_missing_value_does_not()
    {
        Assert.True(Check(AssertionSources.Json, "items[0].none", AssertionOps.Exists).Passed);
        Assert.False(Check(AssertionSources.Json, "items[0].nope", AssertionOps.Exists).Passed);
        Assert.True(Check(AssertionSources.Json, "items[5]", AssertionOps.NotExists).Passed);
    }

    [Fact]
    public void Non_json_body_fails_with_a_clear_message()
    {
        var result = Check(AssertionSources.Json, "a", AssertionOps.Exists, response: Response("<html>", mediaType: "text/html"));
        Assert.False(result.Passed);
        Assert.Contains("not valid JSON", result.Message);
    }

    [Fact]
    public void Malformed_path_and_pattern_fail_clearly_rather_than_throw()
    {
        Assert.Contains("invalid path", Check(AssertionSources.Json, "items[0", AssertionOps.Exists).Message);
        Assert.Contains("invalid path", Check(AssertionSources.Json, "items[x]", AssertionOps.Exists).Message);
        Assert.Contains("invalid pattern", Check(AssertionSources.Status, "", AssertionOps.Matches, "(").Message);
        Assert.Contains("not a number", Check(AssertionSources.Status, "", AssertionOps.LessThan, "soon").Message);
        Assert.Contains("not a number", Check(AssertionSources.Json, "items[0].name", AssertionOps.LessThan, "1").Message);
        Assert.Contains("unknown", Check("nonsense", "", AssertionOps.Equal, "1").Message);
        Assert.Contains("unknown", Check(AssertionSources.Status, "", "nonsense", "1").Message);
    }

    [Fact]
    public void Runaway_pattern_is_stopped()
    {
        var body = $"\"{new string('a', 40)}!\"";
        var result = Check(AssertionSources.Body, "", AssertionOps.Matches, "^(a+)+$", Response(body));
        Assert.False(result.Passed);
    }

    [Fact]
    public void Failure_shows_what_was_received_but_keeps_it_short()
    {
        var long_body = new string('x', 1000);
        var result = Check(AssertionSources.Body, "", AssertionOps.Equal, "y", Response(long_body, mediaType: "text/plain"));
        Assert.StartsWith("got xxx", result.Message);
        Assert.True(result.Message.Length < 300);
    }

    [Fact]
    public void Disabled_assertions_are_skipped()
    {
        var results = AssertionEvaluator.Evaluate([new RequestAssertion { Enabled = false, Expected = "999" }], Response());
        Assert.Empty(results);
    }

    [Fact]
    public void Expected_values_can_use_variables()
    {
        var request = new ApiRequest { Assertions = [new RequestAssertion { Expected = "{{code}}" }] };
        var resolved = VariableResolver.Apply(request, [new("code", "200")]);
        Assert.True(AssertionEvaluator.Evaluate(resolved.Assertions, Response()).Single().Passed);
    }

    [Fact]
    public void Requests_without_assertions_write_nothing_about_them()
    {
        var root = Directory.CreateTempSubdirectory("ricochet-assert").FullName;
        var folder = new WorkspaceFolder(root);
        folder.Load();
        folder.Save(new Workspace { Collections = [new Collection { Name = "C", Items = [new CollectionItem { Name = "R", Request = new ApiRequest { Name = "R" } }] }] });

        var text = string.Concat(Directory.EnumerateFiles(root, "*", SearchOption.AllDirectories).Select(File.ReadAllText));
        Assert.DoesNotContain("assertions", text);
        Directory.Delete(root, recursive: true);
    }

    [Fact]
    public void Assertions_round_trip_through_files()
    {
        var root = Directory.CreateTempSubdirectory("ricochet-assert").FullName;
        var folder = new WorkspaceFolder(root);
        folder.Load();
        var request = new ApiRequest { Name = "R", Assertions = [new RequestAssertion { Source = AssertionSources.Json, Path = "a[0]", Op = AssertionOps.Exists, Enabled = false }] };
        folder.Save(new Workspace { Collections = [new Collection { Name = "C", Items = [new CollectionItem { Name = "R", Request = request }] }] });

        var (loaded, _) = new WorkspaceFolder(root).Load();

        var assertion = loaded.Collections[0].Items[0].Request!.Assertions!.Single();
        Assert.Equal(("a[0]", AssertionOps.Exists, false), (assertion.Path, assertion.Op, assertion.Enabled));
        Directory.Delete(root, recursive: true);
    }
}

public class CaptureEvaluatorTests
{
    private static ApiResponse Response(string body, params KeyValue[] headers) =>
        new(200, "OK", headers, Encoding.UTF8.GetBytes(body), "application/json", null, TimeSpan.Zero, false);

    private static CaptureOutcome Capture(RequestCapture capture, ApiResponse response) =>
        AssertionEvaluator.EvaluateCaptures([capture], response);

    [Fact]
    public void Reads_json_header_and_body()
    {
        var response = Response("""{"data":{"token":"abc","count":3}}""", new KeyValue("X-Id", "h1"));

        Assert.Equal("abc", Capture(new RequestCapture { Path = "data.token", Variable = "t" }, response).Values.Single().Value);
        Assert.Equal("3", Capture(new RequestCapture { Path = "data.count", Variable = "c" }, response).Values.Single().Value);
        Assert.Equal("h1", Capture(new RequestCapture { Source = AssertionSources.Header, Path = "x-id", Variable = "h" }, response).Values.Single().Value);
        Assert.Contains("token", Capture(new RequestCapture { Source = AssertionSources.Body, Variable = "b" }, response).Values.Single().Value);
    }

    [Theory]
    [InlineData("""{"a":1}""", "b", "t", "not found")]
    [InlineData("""{"b":null}""", "b", "t", "not found")]
    [InlineData("<html>", "b", "t", "not valid JSON")]
    [InlineData("""{"a":1}""", "a[", "t", "invalid path")]
    [InlineData("""{"a":1}""", "a", " ", "no variable name")]
    public void Finding_nothing_is_a_failure(string body, string path, string variable, string expected)
    {
        var outcome = Capture(new RequestCapture { Path = path, Variable = variable }, Response(body));

        Assert.Empty(outcome.Values);
        Assert.Contains(expected, outcome.Failures.Single().Message);
    }

    [Fact]
    public void Disabled_captures_are_skipped()
    {
        var outcome = Capture(new RequestCapture { Enabled = false, Path = "nope", Variable = "t" }, Response("{}"));
        Assert.Empty(outcome.Values);
        Assert.Empty(outcome.Failures);
    }

    [Fact]
    public void Captures_round_trip_through_files_and_are_absent_when_unused()
    {
        var root = Directory.CreateTempSubdirectory("ricochet-capture").FullName;
        var folder = new WorkspaceFolder(root);
        folder.Load();
        var request = new ApiRequest { Name = "R", Captures = [new RequestCapture { Path = "data.token", Variable = "token" }] };
        folder.Save(new Workspace { Collections = [new Collection { Name = "C", Items = [new CollectionItem { Name = "R", Request = request }, new CollectionItem { Name = "Plain", Request = new ApiRequest { Name = "Plain" } }] }] });

        var (loaded, _) = new WorkspaceFolder(root).Load();

        Assert.Equal("token", loaded.Collections[0].Items[0].Request!.Captures!.Single().Variable);
        Assert.Null(loaded.Collections[0].Items[1].Request!.Captures);
        var plain = Directory.EnumerateFiles(root, "plain-*.json", SearchOption.AllDirectories).Single();
        Assert.DoesNotContain("captures", File.ReadAllText(plain));
        Directory.Delete(root, recursive: true);
    }
}

public class ChainedRunTests
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

    private const string Token = "captured-token-value";

    private static HttpResponseMessage LoginOrEcho(HttpRequestMessage request) => new(HttpStatusCode.OK)
    {
        Content = new StringContent(request.RequestUri!.AbsolutePath == "/login"
            ? $$$"""{"data":{"token":"{{{Token}}}"}}"""
            : $"you sent: {request.Headers.Authorization}"),
    };

    private static CollectionItem Login(string path = "data.token") => new()
    {
        Name = "Login",
        Request = new ApiRequest { Name = "Login", Method = "POST", Url = "http://x.test/login", Captures = [new RequestCapture { Path = path, Variable = "token" }] },
    };

    private static CollectionItem Me(params RequestAssertion[] assertions) => new()
    {
        Name = "Me",
        Request = new ApiRequest
        {
            Name = "Me",
            Url = "http://x.test/me",
            Auth = new RequestAuth { Kind = AuthKinds.Bearer, Token = "{{token}}" },
            Assertions = assertions.Length > 0 ? assertions : null,
        },
    };

    private static (CollectionRunner Runner, Server Server) Runner()
    {
        var server = new Server(LoginOrEcho);
        return (new CollectionRunner(new RequestSender(new HttpClient(server))), server);
    }

    private static Task<RunReport> Run(CollectionRunner runner, Workspace workspace, RunOptions? options = null) =>
        runner.RunAsync(workspace, options ?? new RunOptions(), cancellationToken: TestContext.Current.CancellationToken);

    private static Workspace One(params CollectionItem[] items) => new() { Collections = [new Collection { Name = "Shop", Items = items }] };

    [Fact]
    public async Task A_later_request_uses_the_captured_value()
    {
        var (runner, server) = Runner();

        var report = await Run(runner, One(Login(), Me()));

        Assert.True(report.Passed);
        Assert.Equal($"Bearer {Token}", server.Received[1].Headers.Authorization!.ToString());
        Assert.Equal(["token"], report.Results[0].Captured);
    }

    [Fact]
    public async Task An_earlier_request_cannot_use_a_value_captured_after_it()
    {
        var (runner, server) = Runner();

        var report = await Run(runner, One(Me(), Login()));

        Assert.False(report.Results[0].Passed);
        Assert.Contains("token", report.Results[0].Error);
        Assert.Single(server.Received);
    }

    [Fact]
    public async Task Failed_capture_fails_its_request_and_stops_dependents_being_sent()
    {
        var (runner, server) = Runner();

        var report = await Run(runner, One(Login("nope"), Me()));

        Assert.False(report.Results[0].Passed);
        Assert.Contains("capture token", report.Results[0].Assertions.Single().Description);
        Assert.Contains("no value for: token", report.Results[1].Error);
        Assert.Single(server.Received);
    }

    [Fact]
    public async Task Captured_values_are_masked_in_results()
    {
        var (runner, _) = Runner();

        var report = await Run(runner, One(Login(), Me(new RequestAssertion { Source = AssertionSources.Body, Expected = "nope" })));

        var failure = report.Results[1].Assertions.Single();
        Assert.False(failure.Passed);
        Assert.DoesNotContain(Token, failure.Message);
        Assert.Contains("***", failure.Message);
    }

    [Fact]
    public async Task Captures_override_scopes_but_not_values_given_for_the_run()
    {
        var (runner, server) = Runner();
        var workspace = new Workspace
        {
            Globals = [new("token", "global-token")],
            Environments = [new ApiEnvironment { Name = "E", Variables = [new("token", "env-token")] }],
            Collections = [new Collection { Name = "Shop", Variables = [new("token", "coll-token")], Items = [Login(), Me()] }],
        };

        await Run(runner, workspace, new RunOptions { EnvironmentName = "E" });
        Assert.Equal($"Bearer {Token}", server.Received[1].Headers.Authorization!.ToString());

        var (second, secondServer) = Runner();
        await Run(second, workspace, new RunOptions { EnvironmentName = "E", Overrides = [new("token", "from-command-line")] });
        Assert.Equal("Bearer from-command-line", secondServer.Received[1].Headers.Authorization!.ToString());
    }

    [Fact]
    public async Task Captures_do_not_leak_into_the_next_collection()
    {
        var (runner, server) = Runner();
        var workspace = new Workspace
        {
            Collections =
            [
                new Collection { Name = "A", Items = [Login()] },
                new Collection { Name = "B", Items = [Me()] },
            ],
        };

        var report = await Run(runner, workspace);

        Assert.Single(server.Received);
        Assert.Contains("token", report.Results[1].Error);
    }

    [Fact]
    public async Task Capture_only_request_is_not_called_unchecked()
    {
        var (runner, _) = Runner();

        var report = await Run(runner, One(Login()));

        Assert.Equal(0, report.RequestsWithoutAssertions);
    }

    [Fact]
    public async Task Captured_values_never_reach_the_workspace_files()
    {
        var root = Directory.CreateTempSubdirectory("ricochet-capture").FullName;
        var folder = new WorkspaceFolder(root);
        folder.Load();
        folder.Save(One(Login(), Me()));
        var (loaded, _) = folder.Load();
        var (runner, _) = Runner();

        await Run(runner, loaded);
        folder.Save(loaded);

        var text = string.Concat(Directory.EnumerateFiles(root, "*", SearchOption.AllDirectories).Select(File.ReadAllText));
        Assert.DoesNotContain(Token, text);
        Directory.Delete(root, recursive: true);
    }
}

public class CollectionRunnerTests
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

    private static HttpResponseMessage Ok(string body = """{"ok":true}""") => new(HttpStatusCode.OK) { Content = new StringContent(body) };

    private static CollectionItem Item(string name, string url = "http://x.test/a", params RequestAssertion[] assertions) =>
        new() { Name = name, Request = new ApiRequest { Name = name, Url = url, Assertions = assertions.Length > 0 ? assertions : null } };

    private static CollectionItem Folder(string name, params CollectionItem[] children) => new() { Name = name, Children = children };

    private static RequestAssertion StatusIs(string status) => new() { Expected = status };

    private static (CollectionRunner Runner, Server Server) Runner(Func<HttpRequestMessage, HttpResponseMessage>? respond = null)
    {
        var server = new Server(respond ?? (_ => Ok()));
        return (new CollectionRunner(new RequestSender(new HttpClient(server))), server);
    }

    private static Workspace One(params CollectionItem[] items) => new() { Collections = [new Collection { Name = "Shop", Items = items }] };

    private static Task<RunReport> Run(CollectionRunner runner, Workspace workspace, RunOptions? options = null) =>
        runner.RunAsync(workspace, options ?? new RunOptions(), cancellationToken: TestContext.Current.CancellationToken);

    [Fact]
    public async Task Runs_every_request_in_order_including_folders()
    {
        var (runner, server) = Runner();
        var report = await Run(runner, One(Item("1", "http://x.test/1"), Folder("F", Item("2", "http://x.test/2")), Item("3", "http://x.test/3")));

        Assert.True(report.Passed);
        Assert.Equal(["/1", "/2", "/3"], server.Received.Select(r => r.RequestUri!.AbsolutePath));
        Assert.Equal("F / 2", report.Results[1].Path);
    }

    [Fact]
    public async Task Failed_assertion_fails_the_run_but_later_requests_still_run()
    {
        var (runner, server) = Runner();
        var report = await Run(runner, One(Item("1", "http://x.test/1", StatusIs("500")), Item("2", "http://x.test/2")));

        Assert.False(report.Passed);
        Assert.Equal(2, server.Received.Count);
        Assert.Equal(1, report.FailedAssertions);
    }

    [Fact]
    public async Task Bail_stops_at_the_first_failure_and_says_so()
    {
        var (runner, server) = Runner();
        var report = await Run(runner, One(Item("1", "http://x.test/1", StatusIs("500")), Item("2", "http://x.test/2")), new RunOptions { StopOnFirstFailure = true });

        Assert.Single(server.Received);
        Assert.True(report.Stopped);
        Assert.Contains(RunReports.Cautions(report), c => c.Contains("Stopped"));
    }

    [Fact]
    public async Task Network_failure_is_a_failed_request_not_a_crash()
    {
        var (runner, _) = Runner(_ => throw new HttpRequestException("No such host"));
        var report = await Run(runner, One(Item("1")));

        Assert.False(report.Passed);
        Assert.Equal("No such host", report.Results[0].Error);
    }

    [Fact]
    public async Task Request_with_a_missing_variable_is_not_sent()
    {
        var (runner, server) = Runner();
        var report = await Run(runner, One(new CollectionItem
        {
            Name = "R",
            Request = new ApiRequest { Name = "R", Url = "http://x.test/", Auth = new RequestAuth { Kind = AuthKinds.Bearer, Token = "{{token}}" } },
        }));

        Assert.Empty(server.Received);
        Assert.False(report.Passed);
        Assert.Contains("token", report.Results[0].Error);
    }

    [Fact]
    public async Task A_request_that_was_not_sent_is_not_called_assertion_free()
    {
        var (runner, _) = Runner();
        var report = await Run(runner, One(Item("Unsent", "{{missing}}")));

        Assert.Equal(0, report.RequestsWithoutAssertions);
        Assert.DoesNotContain(RunReports.Cautions(report), c => c.Contains("no assertions"));
    }

    [Fact]
    public async Task Scopes_layer_in_order_with_overrides_on_top()
    {
        var (runner, server) = Runner();
        var workspace = new Workspace
        {
            Globals = [new("a", "global"), new("b", "global"), new("c", "global"), new("d", "global")],
            Environments = [new ApiEnvironment { Name = "Staging", Variables = [new("c", "env"), new("d", "env")] }],
            Collections =
            [
                new Collection
                {
                    Name = "Shop",
                    Variables = [new("b", "coll"), new("c", "coll"), new("d", "coll")],
                    Items = [Item("R", "http://x.test/?a={{a}}&b={{b}}&c={{c}}&d={{d}}")],
                },
            ],
        };

        await Run(runner, workspace, new RunOptions { EnvironmentName = "staging", Overrides = [new("d", "override")] });

        Assert.Equal("?a=global&b=coll&c=env&d=override", server.Received[0].RequestUri!.Query);
    }

    [Fact]
    public async Task Each_collection_uses_only_its_own_variables()
    {
        var (runner, server) = Runner();
        var workspace = new Workspace
        {
            Collections =
            [
                new Collection { Name = "A", Variables = [new("v", "from-a")], Items = [Item("R", "http://x.test/?v={{v}}")] },
                new Collection { Name = "B", Items = [Item("R", "http://x.test/?v={{v}}")] },
            ],
        };

        var report = await Run(runner, workspace);

        Assert.Single(server.Received);
        Assert.Contains("from-a", server.Received[0].RequestUri!.Query);
        Assert.False(report.Results[1].Passed);
    }

    [Fact]
    public async Task Selects_by_collection_and_folder()
    {
        var (runner, server) = Runner();
        var workspace = new Workspace
        {
            Collections =
            [
                new Collection { Name = "A", Items = [Item("1", "http://x.test/1"), Folder("Smoke", Item("2", "http://x.test/2"), Folder("Deep", Item("3", "http://x.test/3")))] },
                new Collection { Name = "B", Items = [Folder("smoke", Item("4", "http://x.test/4"))] },
            ],
        };

        await Run(runner, workspace, new RunOptions { CollectionName = "a", FolderName = "SMOKE" });

        Assert.Equal(["/2", "/3"], server.Received.Select(r => r.RequestUri!.AbsolutePath));
    }

    [Theory]
    [InlineData("Nope", null, null, "No collection")]
    [InlineData(null, "Nope", null, "No requests")]
    [InlineData(null, null, "Nope", "No environment")]
    public async Task Selecting_nothing_is_an_error_not_a_pass(string? collection, string? folder, string? environment, string expected)
    {
        var (runner, _) = Runner();
        var ex = await Assert.ThrowsAsync<RunSetupException>(() =>
            Run(runner, One(Item("1")), new RunOptions { CollectionName = collection, FolderName = folder, EnvironmentName = environment }));
        Assert.Contains(expected, ex.Message);
    }

    [Fact]
    public async Task An_empty_workspace_is_an_error_not_a_pass()
    {
        var (runner, _) = Runner();
        await Assert.ThrowsAsync<RunSetupException>(() => Run(runner, new Workspace()));
    }

    [Fact]
    public async Task Secret_values_are_masked_in_results()
    {
        var (runner, _) = Runner(_ => Ok("the-secret-value appears in this body"));
        var workspace = new Workspace
        {
            Globals = [new("token", "the-secret-value", IsSecret: true), new("short", "abc", IsSecret: true)],
            Collections =
            [
                new Collection
                {
                    Name = "Shop",
                    Items = [Item("R", "http://x.test/",
                        new RequestAssertion { Source = AssertionSources.Body, Op = AssertionOps.Equal, Expected = "{{token}}" },
                        new RequestAssertion { Source = AssertionSources.Status, Op = AssertionOps.Equal, Expected = "abc" })],
                },
            ],
        };

        var report = await Run(runner, workspace);

        var first = report.Results[0].Assertions[0];
        Assert.DoesNotContain("the-secret-value", first.Message);
        Assert.DoesNotContain("the-secret-value", first.Description);
        Assert.Contains("***", first.Message);
    }

    [Fact]
    public async Task Overrides_and_auth_values_are_masked_too()
    {
        var (runner, _) = Runner(_ => Ok("echo override-value and auth-token-value"));
        var workspace = One(new CollectionItem
        {
            Name = "R",
            Request = new ApiRequest
            {
                Name = "R",
                Url = "http://x.test/",
                Auth = new RequestAuth { Kind = AuthKinds.Bearer, Token = "auth-token-value" },
                Assertions = [new RequestAssertion { Source = AssertionSources.Body, Expected = "nope" }],
            },
        });

        var report = await Run(runner, workspace, new RunOptions { Overrides = [new("x", "override-value")] });

        var message = report.Results[0].Assertions[0].Message;
        Assert.DoesNotContain("override-value", message);
        Assert.DoesNotContain("auth-token-value", message);
    }

    [Fact]
    public async Task Skipped_scripts_and_missing_assertions_are_called_out()
    {
        var (runner, _) = Runner();
        var workspace = new Workspace
        {
            Collections =
            [
                new Collection
                {
                    Name = "Shop",
                    Items =
                    [
                        Item("Plain", "http://x.test/", StatusIs("200")),
                        Folder("F", Item("Bare")) with { Scripts = [new StoredScript("test", "pm.x();")] },
                    ],
                },
            ],
        };

        var report = await Run(runner, workspace);

        Assert.True(report.Passed);
        var cautions = RunReports.Cautions(report);
        Assert.Contains(cautions, c => c.Contains("1 request had Postman scripts"));
        Assert.Contains(cautions, c => c.Contains("1 request had no assertions"));
        Assert.False(report.Results[0].HasScripts);
        Assert.True(report.Results[1].HasScripts);
    }

    [Fact]
    public async Task Xunit_report_is_valid_and_records_failures()
    {
        var (runner, _) = Runner(_ => Ok("bad \u0001 char & <tag>"));
        var report = await Run(runner, One(
            Item("Good"),
            Item("Bad <one>", "http://x.test/", new RequestAssertion { Source = AssertionSources.Body, Expected = "nope" }),
            Item("Unsent", "{{missing}}")));

        using var stream = new MemoryStream();
        RunReports.WriteXunit(report, stream, new DateTimeOffset(2026, 11, 4, 9, 30, 0, TimeSpan.Zero));
        stream.Position = 0;
        var xml = XDocument.Load(stream);

        Assert.Equal("assemblies", xml.Root!.Name.LocalName);
        var assembly = Assert.Single(xml.Root.Elements("assembly"));
        Assert.Equal("Shop", assembly.Attribute("name")!.Value);
        Assert.Equal("2026-11-04", assembly.Attribute("run-date")!.Value);
        Assert.Equal("3", assembly.Attribute("total")!.Value);
        Assert.Equal("1", assembly.Attribute("passed")!.Value);
        Assert.Equal("2", assembly.Attribute("failed")!.Value);
        var collection = Assert.Single(assembly.Elements("collection"));
        var tests = collection.Elements("test").ToList();
        Assert.Equal(["Pass", "Fail", "Fail"], tests.Select(t => t.Attribute("result")!.Value));
        Assert.Empty(tests[0].Elements("failure"));
        Assert.Equal("AssertionFailed", tests[1].Element("failure")!.Attribute("exception-type")!.Value);
        Assert.Contains("nope", tests[1].Element("failure")!.Element("message")!.Value);
        Assert.DoesNotContain("\u0001", tests[1].Element("failure")!.Element("message")!.Value, StringComparison.Ordinal);
        Assert.Equal("RequestFailed", tests[2].Element("failure")!.Attribute("exception-type")!.Value);
        Assert.Contains("missing", tests[2].Element("failure")!.Element("message")!.Value);
    }

    [Fact]
    public async Task Runner_does_not_change_the_workspace_on_disk()
    {
        var root = Directory.CreateTempSubdirectory("ricochet-run").FullName;
        var folder = new WorkspaceFolder(root);
        folder.Load();
        folder.Save(One(Item("1")));
        var before = Directory.EnumerateFiles(root, "*", SearchOption.AllDirectories).ToDictionary(f => f, File.ReadAllText);

        var fresh = new WorkspaceFolder(root);
        var (workspace, _) = fresh.Load();
        var (runner, _) = Runner();
        await Run(runner, workspace);

        var after = Directory.EnumerateFiles(root, "*", SearchOption.AllDirectories).ToDictionary(f => f, File.ReadAllText);
        Assert.Equal(before, after);
        Directory.Delete(root, recursive: true);
    }

    [Fact]
    public async Task Cancelling_stops_the_run()
    {
        var (runner, _) = Runner();
        using var cts = new CancellationTokenSource();
        cts.Cancel();

        await Assert.ThrowsAnyAsync<OperationCanceledException>(() => runner.RunAsync(One(Item("1")), new RunOptions(), cancellationToken: cts.Token));
    }
}
