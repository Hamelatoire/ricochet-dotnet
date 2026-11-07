using System.Net;
using Ricochet.Core;

namespace Ricochet.Core.Tests;

public class DotEnvParseTests
{
    [Fact]
    public void Reads_names_and_values_in_order_and_marks_them_as_secrets()
    {
        var file = DotEnv.Parse("API_TOKEN=abc123\nBASE_URL=http://localhost:5000\n");

        Assert.Empty(file.Problems);
        Assert.Equal(["API_TOKEN", "BASE_URL"], file.Variables.Select(v => v.Key));
        Assert.Equal(["abc123", "http://localhost:5000"], file.Variables.Select(v => v.Value));
        Assert.All(file.Variables, v => Assert.True(v.IsSecret));
        Assert.All(file.Variables, v => Assert.True(v.Enabled));
    }

    [Fact]
    public void Skips_blank_lines_comments_and_accepts_export_and_crlf()
    {
        var file = DotEnv.Parse("# local settings\r\n\r\n   # indented comment\r\nexport TOKEN=xyz\r\n  OTHER = spaced  \r\n");

        Assert.Empty(file.Problems);
        Assert.Equal([("TOKEN", "xyz"), ("OTHER", "spaced")], file.Variables.Select(v => (v.Key, v.Value)));
    }

    [Fact]
    public void Unquoted_values_end_at_a_comment_but_keep_a_hash_inside_a_word()
    {
        var file = DotEnv.Parse("A=value # note\nB=pa#ss\nC=\nD=  \n");

        Assert.Equal([("A", "value"), ("B", "pa#ss"), ("C", ""), ("D", "")], file.Variables.Select(v => (v.Key, v.Value)));
    }

    [Fact]
    public void Double_quotes_understand_escapes_and_single_quotes_are_literal()
    {
        var file = DotEnv.Parse("""
            A="say \"hi\"\tnow\\done"
            B='it''s \n literal'
            C="x" # after the quote
            """);

        Assert.Equal(["A", "B", "C"], file.Variables.Select(v => v.Key));
        Assert.Equal("say \"hi\"\tnow\\done", file.Variables[0].Value);
        Assert.Equal("it", file.Variables[1].Value);
        Assert.Equal("x", file.Variables[2].Value);
        Assert.Equal(["Line 2: text after the closing quote of \"B\" was ignored."], file.Problems);
    }

    [Fact]
    public void Quoted_value_can_span_lines_and_later_lines_still_parse()
    {
        var file = DotEnv.Parse("KEY=\"first\nsecond\"\nNEXT=ok\nBAD LINE\n");

        Assert.Equal([("KEY", "first\nsecond"), ("NEXT", "ok")], file.Variables.Select(v => (v.Key, v.Value)));
        Assert.Equal(["Line 4 is not KEY=VALUE, so it was skipped."], file.Problems);
    }

    [Fact]
    public void Repeated_name_keeps_its_first_place_and_its_last_value()
    {
        var file = DotEnv.Parse("A=1\nB=2\nA=3\n");

        Assert.Equal([("A", "3"), ("B", "2")], file.Variables.Select(v => (v.Key, v.Value)));
    }

    [Fact]
    public void Bad_lines_are_reported_and_the_rest_is_kept()
    {
        var file = DotEnv.Parse("=nokey\nno equals here\nbad key=1\nGOOD=yes\nOPEN=\"never closed\nAFTER=1\n");

        Assert.Equal([("GOOD", "yes"), ("AFTER", "1")], file.Variables.Select(v => (v.Key, v.Value)));
        Assert.Equal(
        [
            "Line 1 is not KEY=VALUE, so it was skipped.",
            "Line 2 is not KEY=VALUE, so it was skipped.",
            "Line 3: \"bad key\" is not a usable name, so it was skipped.",
            "Line 5: the quote around \"OPEN\" is never closed, so it was skipped.",
        ], file.Problems);
    }

    [Fact]
    public void A_byte_order_mark_is_ignored()
    {
        var file = DotEnv.Parse("﻿TOKEN=abc");

        Assert.Equal([("TOKEN", "abc")], file.Variables.Select(v => (v.Key, v.Value)));
    }
}

public class DotEnvReadTests : IDisposable
{
    private readonly string _root = Directory.CreateTempSubdirectory("ricochet-dotenv").FullName;

    public void Dispose() => Directory.Delete(_root, recursive: true);

    [Fact]
    public void A_missing_file_gives_no_values_and_no_problems()
    {
        var file = DotEnv.Read(_root);

        Assert.Empty(file.Variables);
        Assert.Empty(file.Problems);
    }

    [Fact]
    public void Reads_the_file_in_the_workspace_folder()
    {
        File.WriteAllText(Path.Combine(_root, DotEnv.FileName), "TOKEN=from-file\n");

        var file = DotEnv.Read(_root);

        Assert.Equal([("TOKEN", "from-file")], file.Variables.Select(v => (v.Key, v.Value)));
    }
}

/// <summary>The .env rules at the run level: values reach requests, yield to Workspace Variables, and never reach the Workspace.</summary>
public class DotEnvRunTests : IDisposable
{
    private const string Secret = "local-only-token-9f3a";

    private readonly string _root = Directory.CreateTempSubdirectory("ricochet-dotenv-run").FullName;

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

    private static Workspace Sample() => new()
    {
        Collections =
        [
            new Collection
            {
                Id = "c1aaaaaaaa",
                Name = "Shop API",
                Variables = [new("base", "http://x.test")],
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
    public async Task A_dotenv_value_is_sent_when_no_workspace_variable_has_the_name()
    {
        var handler = new Handler(_ => new HttpResponseMessage(HttpStatusCode.OK));
        var runner = new CollectionRunner(new RequestSender(new HttpClient(handler)));

        var report = await runner.RunAsync(Sample(), new RunOptions { LocalVariables = [new("API_TOKEN", Secret, IsSecret: true)] },
            cancellationToken: TestContext.Current.CancellationToken);

        Assert.True(report.Passed);
        Assert.Equal($"Bearer {Secret}", handler.Received.Single().Headers.Authorization!.ToString());
    }

    [Fact]
    public async Task A_workspace_variable_with_the_same_name_wins_over_the_dotenv_value()
    {
        var workspace = Sample() with
        {
            Collections = [Sample().Collections[0] with { Variables = [new("base", "http://x.test"), new("API_TOKEN", "from-collection")] }],
        };
        var handler = new Handler(_ => new HttpResponseMessage(HttpStatusCode.OK));
        var runner = new CollectionRunner(new RequestSender(new HttpClient(handler)));

        await runner.RunAsync(workspace, new RunOptions { LocalVariables = [new("API_TOKEN", Secret, IsSecret: true)] },
            cancellationToken: TestContext.Current.CancellationToken);

        Assert.Equal("Bearer from-collection", handler.Received.Single().Headers.Authorization!.ToString());
    }

    [Fact]
    public void Saving_the_workspace_after_a_run_writes_no_dotenv_value_and_leaves_the_file_alone()
    {
        var envPath = Path.Combine(_root, DotEnv.FileName);
        var envText = $"API_TOKEN={Secret}\n";
        File.WriteAllText(envPath, envText);

        var folder = new WorkspaceFolder(_root);
        folder.Load();
        var workspace = Sample();
        folder.Save(workspace);

        // The values are used by a run, then the same Workspace is saved again, as the app does after an edit.
        var dotEnv = DotEnv.Read(_root);
        var resolved = VariableResolver.Layer(dotEnv.Variables, workspace.Globals, workspace.Collections[0].Variables);
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
