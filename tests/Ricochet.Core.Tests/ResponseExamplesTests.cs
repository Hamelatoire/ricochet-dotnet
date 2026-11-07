using System.Text;
using Ricochet.Core;

namespace Ricochet.Core.Tests;

public sealed class ResponseExamplesTests : IDisposable
{
    private readonly string _root = Directory.CreateTempSubdirectory("ricochet-examples").FullName;

    public void Dispose() => Directory.Delete(_root, recursive: true);

    private static ApiResponse Response(string body, IEnumerable<KeyValue>? headers = null, bool truncated = false) => new(
        200,
        "OK",
        (headers ?? []).ToList(),
        Encoding.UTF8.GetBytes(body),
        "application/json",
        null,
        TimeSpan.FromMilliseconds(12),
        truncated);

    [Fact]
    public void Keeps_status_name_and_body_of_a_response()
    {
        var example = ResponseExamples.Create("  Happy path ", new ApiRequest(), [], [],
            Response("{\"id\":1}", [new("Content-Type", "application/json")]));

        Assert.Equal("Happy path", example.Name);
        Assert.Equal(200, example.StatusCode);
        Assert.Equal("OK", example.ReasonPhrase);
        Assert.Equal("application/json", example.MediaType);
        Assert.Contains("\"id\"", example.Body);
        Assert.Equal([new KeyValue("Content-Type", "application/json")], example.Headers);
    }

    [Fact]
    public void Hides_sensitive_response_headers_as_History_does()
    {
        var example = ResponseExamples.Create("Login", new ApiRequest(), [], [], Response("{}",
        [
            new("Set-Cookie", "session=abc123"),
            new("Content-Type", "application/json"),
        ]));

        Assert.Equal(SecretRules.HiddenValue, example.Headers[0].Value);
        Assert.Equal("application/json", example.Headers[1].Value);
    }

    [Fact]
    public void Masks_the_credentials_the_request_sent_wherever_they_are_echoed()
    {
        var sent = new ApiRequest
        {
            Url = "http://x.test/me",
            Headers = [new("Authorization", "Bearer sk-live-9999")],
            Auth = new RequestAuth { Kind = AuthKinds.Bearer, Token = "tok-secret-1234" },
        };

        var example = ResponseExamples.Create("Me", sent, [], [], Response(
            "{\"token\":\"tok-secret-1234\",\"auth\":\"Bearer sk-live-9999\"}",
            [new("X-Echo", "tok-secret-1234")]));

        Assert.DoesNotContain("tok-secret-1234", example.Body);
        Assert.DoesNotContain("sk-live-9999", example.Body);
        Assert.DoesNotContain("tok-secret-1234", example.Headers[0].Value);
        Assert.Contains(SecretRules.HiddenValue, example.Body);
    }

    [Fact]
    public void Masks_secret_variables_and_captured_values()
    {
        var variables = new[] { new KeyValue("pw", "hunter22", IsSecret: true), new KeyValue("plain", "visible-text") };
        var captured = new[] { new KeyValue("session", "cap-value-77") };

        var example = ResponseExamples.Create("Echo", new ApiRequest(), variables, captured,
            Response("hunter22 visible-text cap-value-77"));

        Assert.Equal($"{SecretRules.HiddenValue} visible-text {SecretRules.HiddenValue}", example.Body);
    }

    [Fact]
    public void Keeps_placeholders_that_were_not_resolved()
    {
        var sent = new ApiRequest { Headers = [new("Authorization", "{{token}}")] };

        var example = ResponseExamples.Create("Unresolved", sent, [], [], Response("{{token}}"));

        Assert.Equal("{{token}}", example.Body);
    }

    [Fact]
    public void Refuses_a_truncated_response()
    {
        var ex = Assert.Throws<InvalidOperationException>(() =>
            ResponseExamples.Create("Big", new ApiRequest(), [], [], Response("partial", truncated: true)));

        Assert.Contains("too large", ex.Message);
    }

    [Fact]
    public void Refuses_a_binary_response()
    {
        var binary = new ApiResponse(200, "OK", [], [0, 1, 2, 0, 255, 0, 3], "application/octet-stream", null, TimeSpan.Zero, false);

        Assert.Throws<InvalidOperationException>(() => ResponseExamples.Create("Binary", new ApiRequest(), [], [], binary));
    }

    [Fact]
    public void Examples_are_saved_with_their_request_and_read_back()
    {
        var example = ResponseExamples.Create("Created", new ApiRequest(), [], [], Response("{\"ok\":true}"));
        var request = new ApiRequest { Id = "req1aaaaaa", Name = "Create", Url = "http://x.test/create", Examples = [example] };
        var workspace = new Workspace
        {
            Collections = [new Collection { Id = "c1aaaaaaaa", Name = "Shop", Items = [new CollectionItem { Id = "i1aaaaaaaa", Name = "Create", Request = request }] }],
        };

        var folder = new WorkspaceFolder(_root);
        folder.Load();
        folder.Save(workspace);

        var loaded = new WorkspaceFolder(_root).Load().Workspace;
        var saved = loaded.Collections[0].Items[0].Request!.Examples!;
        Assert.Single(saved);
        Assert.Equal(example.Name, saved[0].Name);
        Assert.Equal(example.Body, saved[0].Body);
        Assert.Equal(example.Id, saved[0].Id);
    }

    [Fact]
    public void A_request_without_examples_adds_nothing_to_its_file()
    {
        var workspace = new Workspace
        {
            Collections = [new Collection { Id = "c1aaaaaaaa", Name = "Shop", Items = [new CollectionItem { Id = "i1aaaaaaaa", Name = "Get", Request = new ApiRequest { Id = "req1aaaaaa", Name = "Get", Url = "http://x.test" } }] }],
        };

        var folder = new WorkspaceFolder(_root);
        folder.Load();
        folder.Save(workspace);

        var files = Directory.EnumerateFiles(_root, "*.json", SearchOption.AllDirectories)
            .Select(File.ReadAllText);
        Assert.All(files, text => Assert.DoesNotContain("examples", text));
    }
}
