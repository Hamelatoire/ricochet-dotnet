using System.Net;
using System.Text;
using Ricochet.Core;

namespace Ricochet.Core.Tests;

public class PostmanImporterTests
{
    private const string V21 = "https://schema.getpostman.com/json/collection/v2.1.0/collection.json";

    private static string CollectionJson(string items, string extra = "") =>
        $$"""{ "info": { "name": "Shop", "schema": "{{V21}}" }, {{extra}} "item": [{{items}}] }""";

    private static ImportResult Import(string json) => PostmanImporter.Import(json, "file.json");

    private static ApiRequest OnlyRequest(ImportResult result) => result.Collection!.Items.Single().Request!;

    private static ApiRequest RequestFrom(string requestJson) =>
        OnlyRequest(Import(CollectionJson($$"""{ "name": "R", "request": {{requestJson}} }""")));

    private static ImportResult ImportRequest(string requestJson) =>
        Import(CollectionJson($$"""{ "name": "R", "request": {{requestJson}} }"""));

    [Fact]
    public void Keeps_folder_order_and_nesting()
    {
        var result = Import(CollectionJson("""
            { "name": "First", "request": { "method": "get", "url": "http://x.test/1" } },
            { "name": "Orders", "item": [
                { "name": "Nested", "item": [ { "name": "Deep", "request": { "method": "POST", "url": "http://x.test/2" } } ] }
            ] },
            { "name": "Last", "request": { "method": "GET", "url": "http://x.test/3" } }
            """));

        var items = result.Collection!.Items;
        Assert.Equal(["First", "Orders", "Last"], items.Select(i => i.Name));
        Assert.Null(items[1].Request);
        Assert.Equal("POST", items[1].Children[0].Children[0].Request!.Method);
        Assert.Equal("GET", items[0].Request!.Method);
        Assert.Contains("3 requests, 2 folders", result.Summary);
    }

    [Fact]
    public void Reads_url_string_and_object_forms()
    {
        Assert.Equal("{{base}}/a", RequestFrom("""{ "url": "{{base}}/a" }""").Url);
        Assert.Equal("{{base}}/b?x=1", RequestFrom("""{ "url": { "raw": "{{base}}/b?x=1" } }""").Url);
        Assert.Equal("https://api.example.com:8443/v1/items?q=1", RequestFrom("""
            { "url": { "protocol": "https", "host": ["api", "example", "com"], "port": "8443", "path": ["v1", "items"],
              "query": [ { "key": "q", "value": "1" }, { "key": "off", "value": "2", "disabled": true } ] } }
            """).Url);
    }

    [Fact]
    public void A_request_can_be_only_a_url_string()
    {
        var request = RequestFrom("\"http://x.test/plain\"");
        Assert.Equal("GET", request.Method);
        Assert.Equal("http://x.test/plain", request.Url);
    }

    [Fact]
    public void Substitutes_path_variables()
    {
        var request = RequestFrom("""
            { "url": { "raw": "http://x.test/users/:id/posts/:postId?a=:id",
                       "variable": [ { "key": "id", "value": "{{userId}}" }, { "key": "postId", "value": "9" } ] } }
            """);
        Assert.Equal("http://x.test/users/{{userId}}/posts/9?a=:id", request.Url);
    }

    [Fact]
    public void Disabled_query_parameter_is_reported()
    {
        var result = ImportRequest("""{ "url": { "raw": "http://x.test", "query": [ { "key": "off", "value": "1", "disabled": true } ] } }""");
        Assert.Contains(result.Issues, i => i.Category == ImportCategories.DroppedData && i.Detail.Contains("off"));
    }

    [Fact]
    public void Reads_headers_including_disabled_and_string_form()
    {
        var list = RequestFrom("""{ "url": "u", "header": [ { "key": "A", "value": "1" }, { "key": "B", "value": "2", "disabled": true } ] }""");
        Assert.True(list.Headers[0].Enabled);
        Assert.False(list.Headers[1].Enabled);

        var text = RequestFrom("""{ "url": "u", "header": "A: 1\nB: two: parts\n" }""");
        Assert.Equal("two: parts", text.Headers[1].Value);
    }

    [Fact]
    public void Raw_body_gets_content_type_from_its_language_unless_already_set()
    {
        var json = RequestFrom("""{ "url": "u", "body": { "mode": "raw", "raw": "{}", "options": { "raw": { "language": "json" } } } }""");
        Assert.Equal("{}", json.Body);
        Assert.Equal("application/json", json.Headers.Single(h => h.Key == "Content-Type").Value);

        var explicitType = RequestFrom("""
            { "url": "u", "header": [ { "key": "content-type", "value": "text/x" } ],
              "body": { "mode": "raw", "raw": "{}", "options": { "raw": { "language": "json" } } } }
            """);
        Assert.Equal("text/x", explicitType.Headers.Single().Value);
    }

    [Fact]
    public void Urlencoded_body_escapes_values_but_keeps_placeholders()
    {
        var result = ImportRequest("""
            { "url": "u", "body": { "mode": "urlencoded", "urlencoded": [
                { "key": "a b", "value": "x&y" }, { "key": "t", "value": "pre {{token}}" }, { "key": "off", "value": "1", "disabled": true } ] } }
            """);

        var request = OnlyRequest(result);
        Assert.Equal("a%20b=x%26y&t=pre%20{{token}}", request.Body);
        Assert.Equal("application/x-www-form-urlencoded", request.Headers.Single().Value);
        Assert.Contains(result.Issues, i => i.Category == ImportCategories.DroppedBody && i.Detail.Contains("off"));
    }

    [Fact]
    public async Task Formdata_text_fields_become_multipart_a_server_can_parse_and_files_are_reported()
    {
        var result = ImportRequest("""
            { "url": "u", "body": { "mode": "formdata", "formdata": [
                { "key": "title", "value": "hello", "type": "text" },
                { "key": "upload", "type": "file", "src": "/tmp/a.png" } ] } }
            """);

        var request = OnlyRequest(result);
        var contentType = request.Headers.Single(h => h.Key == "Content-Type").Value;
        using var content = new ByteArrayContent(Encoding.UTF8.GetBytes(request.Body));
        content.Headers.TryAddWithoutValidation("Content-Type", contentType);
        Assert.Contains("name=\"title\"", await content.ReadAsStringAsync(TestContext.Current.CancellationToken));
        Assert.DoesNotContain("upload", request.Body);
        Assert.EndsWith("--\r\n", request.Body);
        Assert.Contains(result.Issues, i => i.Category == ImportCategories.DroppedBody && i.Detail.Contains("upload"));
    }

    [Fact]
    public void Graphql_body_becomes_json()
    {
        var request = RequestFrom("""
            { "url": "u", "body": { "mode": "graphql", "graphql": { "query": "{ me { id } }", "variables": "{\"a\":1}" } } }
            """);
        Assert.Contains("\"query\": \"{ me { id } }\"", request.Body);
        Assert.Contains("\"a\": 1", request.Body);
        Assert.Equal("application/json", request.Headers.Single().Value);
    }

    [Theory]
    [InlineData("""{ "mode": "file", "file": { "src": "a.png" } }""")]
    [InlineData("""{ "mode": "somethingnew" }""")]
    [InlineData("""{ "mode": "raw", "raw": "x", "disabled": true }""")]
    public void Bodies_that_cannot_be_carried_over_are_reported(string body)
    {
        var result = ImportRequest($$"""{ "url": "u", "body": {{body}} }""");
        Assert.Equal("", OnlyRequest(result).Body);
        Assert.Contains(result.Issues, i => i.Category == ImportCategories.DroppedBody);
    }

    [Fact]
    public void Reads_auth_in_both_v21_and_v20_shapes()
    {
        var v21 = RequestFrom("""{ "url": "u", "auth": { "type": "bearer", "bearer": [ { "key": "token", "value": "{{t}}" } ] } }""");
        Assert.Equal("{{t}}", v21.Auth!.Token);

        var v20 = RequestFrom("""{ "url": "u", "auth": { "type": "basic", "basic": { "username": "me", "password": "pw" } } }""");
        Assert.Equal(("me", "pw"), (v20.Auth!.Username, v20.Auth.Password));

        var apiKey = RequestFrom("""
            { "url": "u", "auth": { "type": "apikey", "apikey": [ { "key": "key", "value": "k" }, { "key": "value", "value": "v" }, { "key": "in", "value": "query" } ] } }
            """);
        Assert.Equal(("k", "v", true), (apiKey.Auth!.KeyName, apiKey.Auth.KeyValue, apiKey.Auth.KeyInQuery));
    }

    [Fact]
    public void Auth_inherits_from_nearest_parent_and_noauth_stops_it()
    {
        var result = Import(CollectionJson("""
            { "name": "FromCollection", "request": { "url": "u" } },
            { "name": "Folder", "auth": { "type": "bearer", "bearer": [ { "key": "token", "value": "folder" } ] }, "item": [
                { "name": "FromFolder", "request": { "url": "u" } },
                { "name": "Explicit", "request": { "url": "u", "auth": { "type": "inherit" } } },
                { "name": "NoAuth", "request": { "url": "u", "auth": { "type": "noauth" } } }
            ] }
            """, """ "auth": { "type": "bearer", "bearer": [ { "key": "token", "value": "root" } ] }, """));

        var items = result.Collection!.Items;
        Assert.Equal("root", items[0].Request!.Auth!.Token);
        Assert.Equal("folder", items[1].Children[0].Request!.Auth!.Token);
        Assert.Equal("folder", items[1].Children[1].Request!.Auth!.Token);
        Assert.Null(items[1].Children[2].Request!.Auth);
    }

    [Fact]
    public void Unsupported_auth_is_reported()
    {
        var result = ImportRequest("""{ "url": "u", "auth": { "type": "oauth2", "oauth2": [] } }""");
        var issue = Assert.Single(result.Issues);
        Assert.Equal(ImportCategories.UnsupportedAuth, issue.Category);
        Assert.Equal("oauth2", issue.Detail);
        Assert.Null(OnlyRequest(result).Auth);
    }

    [Fact]
    public void Scripts_are_kept_and_reported_at_every_level_but_empty_stubs_are_ignored()
    {
        const string real = """ "event": [ { "listen": "prerequest", "script": { "exec": ["pm.a();", "pm.b();"] } } ], """;
        const string stub = """ "event": [ { "listen": "test", "script": { "exec": [""] } } ], """;
        var result = Import(CollectionJson($$"""
            { "name": "Folder", {{real}} "item": [
                { "name": "WithScript", {{real}} "request": { "url": "u" } },
                { "name": "WithStub", {{stub}} "request": { "url": "u" } }
            ] }
            """, real));

        Assert.Equal("pm.a();\npm.b();", result.Collection!.Scripts.Single().Source);
        var folder = result.Collection.Items[0];
        Assert.Single(folder.Scripts);
        Assert.Single(folder.Children[0].Scripts);
        Assert.Empty(folder.Children[1].Scripts);

        var reported = result.Issues.Where(i => i.Category == ImportCategories.Scripts).Select(i => i.Location).ToList();
        Assert.Equal(3, reported.Count);
        Assert.Contains("Shop / Folder / WithScript", reported);
        Assert.DoesNotContain("Shop / Folder / WithStub", reported);
    }

    [Fact]
    public void Dynamic_variables_stay_as_written_and_are_reported()
    {
        var result = ImportRequest("""{ "url": "http://x.test/{{$guid}}", "body": { "mode": "raw", "raw": "{{$timestamp}} {{normal}}" } }""");
        Assert.Equal("http://x.test/{{$guid}}", OnlyRequest(result).Url);
        var issue = result.Issues.Single(i => i.Category == ImportCategories.DynamicVariables);
        Assert.Contains("$guid", issue.Detail);
        Assert.Contains("$timestamp", issue.Detail);
        Assert.DoesNotContain("normal", issue.Detail);
    }

    [Fact]
    public void Examples_and_descriptions_are_counted_as_not_imported()
    {
        var result = ImportRequest("""{ "url": "u", "description": "d" }""");
        Assert.Contains(result.Issues, i => i.Location == "1 description");

        var examples = Import(CollectionJson("""{ "name": "R", "request": { "url": "u" }, "response": [ { "name": "e1" }, { "name": "e2" } ] }"""));
        Assert.Contains(examples.Issues, i => i.Location.Contains("2 saved example"));
    }

    [Fact]
    public void Reads_collection_variables_with_non_string_values()
    {
        var result = Import(CollectionJson("", """
            "variable": [ { "key": "a", "value": "1" }, { "key": "n", "value": 5 }, { "id": "byId", "value": true },
                          { "key": "off", "value": "x", "disabled": true }, { "key": "s", "value": "pw", "type": "secret" } ],
            """));

        var variables = result.Collection!.Variables;
        Assert.Equal("5", variables[1].Value);
        Assert.Equal(("byId", "true"), (variables[2].Key, variables[2].Value));
        Assert.False(variables[3].Enabled);
        Assert.Contains(result.Issues, i => i.Category == ImportCategories.Secrets && i.Detail == "s");
    }

    [Fact]
    public void Reads_environment_files()
    {
        var result = Import("""
            { "name": "Staging", "_postman_variable_scope": "environment", "values": [
                { "key": "base", "value": "http://s.test", "enabled": true },
                { "key": "off", "value": "1", "enabled": false },
                { "key": "token", "value": "", "type": "secret", "enabled": true } ] }
            """);

        Assert.Equal("Staging", result.Environment!.Name);
        Assert.False(result.Environment.Variables[1].Enabled);
        Assert.Contains(result.Issues, i => i.Category == ImportCategories.Secrets && i.Detail.Contains("no value"));
        Assert.Null(result.Globals);
    }

    [Fact]
    public void Reads_globals_files()
    {
        var result = Import("""{ "name": "g", "_postman_variable_scope": "globals", "values": [ { "key": "a", "value": "1" } ] }""");
        Assert.Equal("a", result.Globals!.Single().Key);
        Assert.Null(result.Environment);
    }

    [Theory]
    [InlineData("{not json", "Not valid JSON")]
    [InlineData("[]", "not a Postman")]
    [InlineData("""{ "hello": 1 }""", "not a Postman")]
    [InlineData("""{ "version": 1, "collections": [] }""", "v1")]
    [InlineData("""{ "info": { "schema": "https://schema.getpostman.com/json/collection/v1.0.0/collection.json" }, "item": [] }""", "Unsupported")]
    public void Unreadable_files_give_a_clear_error(string json, string expected)
    {
        var ex = Assert.Throws<PostmanImportException>(() => Import(json));
        Assert.Contains(expected, ex.Message);
    }

    [Theory]
    [InlineData("""{ "info": 1, "item": 1 }""")]
    [InlineData("""{ "info": { "name": 5 }, "item": [ 1, "x", null, { "name": 7 } ] }""")]
    [InlineData("""{ "info": {}, "item": [ { "name": "R", "request": { "url": 5, "header": 5, "body": 5, "auth": 5, "method": null } } ] }""")]
    public void Malformed_structure_does_not_crash(string json)
    {
        Assert.NotNull(Import(json).Collection);
    }

    [Fact]
    public void Report_groups_issues_by_category()
    {
        var report = ImportRequest("""{ "url": "u", "auth": { "type": "oauth2" } }""").ToReportText();
        Assert.Contains("Imported collection \"Shop\"", report);
        Assert.Contains($"{ImportCategories.UnsupportedAuth} (1):", report);
        Assert.Contains("Shop / R: oauth2", report);
    }

    [Fact]
    public async Task Imported_request_sends_with_scoped_variables_applied()
    {
        var result = Import(CollectionJson("""
            { "name": "R", "request": { "method": "POST", "url": "{{base}}/items",
              "auth": { "type": "bearer", "bearer": [ { "key": "token", "value": "{{token}}" } ] },
              "body": { "mode": "raw", "raw": "{\"who\":\"{{who}}\"}", "options": { "raw": { "language": "json" } } } } }
            """, """ "variable": [ { "key": "base", "value": "http://collection.test" }, { "key": "who", "value": "collection" } ], """));

        var variables = VariableResolver.Layer(
            [new KeyValue("token", "global"), new KeyValue("who", "global")],
            result.Collection!.Variables,
            [new KeyValue("base", "http://env.test")]);

        HttpRequestMessage? received = null;
        string? body = null;
        var handler = new CapturingHandler(async (message, ct) =>
        {
            received = message;
            body = await message.Content!.ReadAsStringAsync(ct);
        });
        var sender = new RequestSender(new HttpClient(handler));

        await sender.SendAsync(VariableResolver.Apply(OnlyRequest(result), variables), TestContext.Current.CancellationToken);

        Assert.Equal("http://env.test/items", received!.RequestUri!.ToString());
        Assert.Equal("Bearer global", received.Headers.Authorization!.ToString());
        Assert.Equal("{\"who\":\"collection\"}", body);
        Assert.Equal("application/json", received.Content!.Headers.ContentType!.MediaType);
    }

    private sealed class CapturingHandler(Func<HttpRequestMessage, CancellationToken, Task> capture) : HttpMessageHandler
    {
        protected override async Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken)
        {
            await capture(request, cancellationToken);
            return new HttpResponseMessage(HttpStatusCode.OK);
        }
    }
}

public class WorkspaceImportTests
{
    [Fact]
    public void Name_clashes_get_a_number()
    {
        Assert.Equal("A", WorkspaceImport.UniqueName("A", ["B"]));
        Assert.Equal("A (2)", WorkspaceImport.UniqueName("A", ["a"]));
        Assert.Equal("A (3)", WorkspaceImport.UniqueName("A", ["A", "A (2)"]));
    }

    [Fact]
    public void Merging_globals_reports_changed_values_only()
    {
        var (merged, overwritten) = WorkspaceImport.MergeGlobals(
            [new("same", "1"), new("changed", "old")],
            [new("same", "1"), new("changed", "new"), new("added", "x")]);

        Assert.Equal(["same", "changed", "added"], merged.Select(v => v.Key));
        Assert.Equal("new", merged[1].Value);
        Assert.Equal(["changed"], overwritten);
    }
}

public class ScopedVariableTests
{
    [Fact]
    public void Environment_beats_collection_beats_globals()
    {
        var variables = VariableResolver.Layer(
            [new("a", "global"), new("b", "global"), new("c", "global")],
            [new("b", "collection"), new("c", "collection")],
            [new("c", "environment")]);

        var resolved = VariableResolver.Apply(new ApiRequest { Url = "{{a}} {{b}} {{c}}" }, variables);

        Assert.Equal("global collection environment", resolved.Url);
    }

    [Fact]
    public void Disabled_variable_does_not_hide_a_lower_scope()
    {
        var variables = VariableResolver.Layer([new("a", "global")], [new("a", "env", Enabled: false)]);
        Assert.Equal("global", VariableResolver.Apply(new ApiRequest { Url = "{{a}}" }, variables).Url);
    }

    [Fact]
    public void Values_can_reference_other_variables_and_cycles_end()
    {
        var nested = VariableResolver.Apply(new ApiRequest { Url = "{{url}}" }, [new("url", "{{host}}/api"), new("host", "x.test")]);
        Assert.Equal("x.test/api", nested.Url);

        var cycle = VariableResolver.Apply(new ApiRequest { Url = "{{a}}" }, [new("a", "{{b}}"), new("b", "{{a}}")]);
        Assert.Contains("{{", cycle.Url);
    }

    [Fact]
    public void Names_may_contain_spaces()
    {
        Assert.Equal("1", VariableResolver.Apply(new ApiRequest { Url = "{{my var}}" }, [new("my var", "1")]).Url);
    }

    [Fact]
    public void Auth_fields_are_resolved()
    {
        var request = new ApiRequest { Auth = new RequestAuth { Kind = AuthKinds.Bearer, Token = "{{t}}" } };
        Assert.Equal("abc", VariableResolver.Apply(request, [new("t", "abc")]).Auth!.Token);
    }

    [Fact]
    public void Finds_placeholders_left_in_what_will_be_sent()
    {
        var request = new ApiRequest
        {
            Url = "{{a}}/{{$guid}}",
            Headers = [new("X", "{{b}}"), new("Y", "{{off}}", Enabled: false)],
            Auth = new RequestAuth { Token = "{{a}}" },
        };

        Assert.Equal(["a", "$guid", "b"], VariableResolver.FindPlaceholders(request));
    }
}

public class AuthSendingTests
{
    private sealed class Capture : HttpMessageHandler
    {
        public HttpRequestMessage? Received { get; private set; }

        protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken)
        {
            Received = request;
            return Task.FromResult(new HttpResponseMessage(HttpStatusCode.OK));
        }
    }

    private static async Task<HttpRequestMessage> Send(RequestAuth auth, string url = "http://x.test/", params KeyValue[] headers)
    {
        var capture = new Capture();
        await new RequestSender(new HttpClient(capture)).SendAsync(
            new ApiRequest { Url = url, Auth = auth, Headers = headers },
            TestContext.Current.CancellationToken);
        return capture.Received!;
    }

    [Fact]
    public async Task Bearer_and_basic_set_authorization()
    {
        var bearer = await Send(new RequestAuth { Kind = AuthKinds.Bearer, Token = "abc" });
        Assert.Equal("Bearer abc", bearer.Headers.Authorization!.ToString());

        var basic = await Send(new RequestAuth { Kind = AuthKinds.Basic, Username = "me", Password = "pw" });
        Assert.Equal("Basic bWU6cHc=", basic.Headers.Authorization!.ToString());
    }

    [Fact]
    public async Task Api_key_goes_in_header_or_query()
    {
        var header = await Send(new RequestAuth { Kind = AuthKinds.ApiKey, KeyName = "X-Key", KeyValue = "v" });
        Assert.Equal("v", header.Headers.GetValues("X-Key").Single());

        var query = await Send(new RequestAuth { Kind = AuthKinds.ApiKey, KeyName = "k y", KeyValue = "a&b", KeyInQuery = true }, "http://x.test/p?z=1");
        Assert.Equal("http://x.test/p?z=1&k%20y=a%26b", query.RequestUri!.AbsoluteUri);
    }

    [Fact]
    public async Task Explicit_header_wins_over_auth()
    {
        var sent = await Send(new RequestAuth { Kind = AuthKinds.Bearer, Token = "auth" }, "http://x.test/", new KeyValue("Authorization", "Custom own"));
        Assert.Equal("Custom own", sent.Headers.GetValues("Authorization").Single());
    }
}

public sealed class WorkspaceMigrationTests : IDisposable
{
    private readonly string _dir = Directory.CreateTempSubdirectory("ricochet-migration").FullName;

    public void Dispose() => Directory.Delete(_dir, recursive: true);

    [Fact]
    public void Data_from_the_old_app_name_is_copied_once_and_never_overwrites()
    {
        var oldPath = Path.Combine(_dir, "PostmanLight", "workspace.json");
        var currentPath = Path.Combine(_dir, "Ricochet", "workspace.json");

        WorkspaceStore.AdoptFileFromOldName(oldPath, currentPath);
        Assert.False(File.Exists(currentPath));

        Directory.CreateDirectory(Path.GetDirectoryName(oldPath)!);
        File.WriteAllText(oldPath, "old");
        WorkspaceStore.AdoptFileFromOldName(oldPath, currentPath);
        Assert.Equal("old", File.ReadAllText(currentPath));
        Assert.True(File.Exists(oldPath));

        File.WriteAllText(currentPath, "new");
        WorkspaceStore.AdoptFileFromOldName(oldPath, currentPath);
        Assert.Equal("new", File.ReadAllText(currentPath));
    }

    [Fact]
    public void Original_file_format_is_migrated_without_losing_data()
    {
        var path = Path.Combine(_dir, "workspace.json");
        File.WriteAllText(path, """
            { "saved": [ { "id": "r1", "name": "Ping", "method": "GET", "url": "http://x.test", "headers": [], "body": "" } ],
              "history": [ { "request": { "id": "h", "url": "u" }, "statusCode": 200, "elapsedMs": 1, "at": "2026-11-04T00:00:00+00:00" } ],
              "variables": [ { "key": "a", "value": "1", "enabled": true } ] }
            """);
        var store = new LegacyWorkspaceFile(path);

        var loaded = store.Load();

        var collection = Assert.Single(loaded.Collections);
        Assert.Equal(Workspace.DefaultCollectionName, collection.Name);
        Assert.Equal("r1", collection.Items.Single().Request!.Id);
        Assert.Equal("a", loaded.Globals.Single().Key);
        Assert.Empty(loaded.History.Single().Request.Headers); // Missing from the file, so the default applies.
        Assert.Equal("", loaded.History.Single().Request.Body);

    }
}
