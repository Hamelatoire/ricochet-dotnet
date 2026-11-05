using System.Net;
using System.Text;
using Ricochet.Core;

namespace Ricochet.Core.Tests;

public class OpenApiTests
{
    private const string Url = "https://api.example.test/openapi.json";

    private const string Spec = """
        {
          "openapi": "3.0.3",
          "info": { "title": "Pets" },
          "servers": [ { "url": "https://api.example.test/v1/" } ],
          "security": [ { "bearerAuth": [] } ],
          "paths": {
            "/pets": {
              "get": { "operationId": "listPets", "summary": "List pets", "tags": ["pets"],
                "parameters": [ { "name": "limit", "in": "query", "schema": { "type": "integer" } } ] },
              "post": { "operationId": "createPet", "summary": "Create a pet", "tags": ["pets"],
                "requestBody": { "content": { "application/json": { "schema": { "$ref": "#/components/schemas/Pet" } } } } }
            },
            "/pets/{petId}": {
              "delete": { "summary": "Delete a pet", "tags": ["pets"], "security": [ { "apiKeyAuth": [] } ] }
            },
            "/health": {
              "get": { "operationId": "health", "summary": "Health", "security": [] }
            }
          },
          "components": {
            "schemas": {
              "Pet": {
                "type": "object",
                "properties": {
                  "name": { "type": "string" },
                  "age": { "type": "integer" },
                  "tags": { "type": "array", "items": { "type": "string" } }
                }
              }
            },
            "securitySchemes": {
              "bearerAuth": { "type": "http", "scheme": "bearer" },
              "apiKeyAuth": { "type": "apiKey", "in": "header", "name": "X-Api-Key" }
            }
          }
        }
        """;

    private static OpenApiResult Import(string json = Spec, Collection? existing = null) =>
        OpenApiImporter.Build(json, Url, existing);

    private static CollectionItem Folder(Collection collection, string name) =>
        collection.Items.Single(i => i.Request is null && i.Name == name);

    [Fact]
    public void Creates_a_collection_with_a_folder_per_tag_and_the_server_as_a_variable()
    {
        var collection = Import().Collection;

        Assert.Equal("Pets", collection.Name);
        Assert.Equal(Url, collection.SpecUrl);
        Assert.Equal("https://api.example.test/v1", collection.Variables.Single(v => v.Key == "baseUrl").Value);

        var pets = Folder(collection, "pets");
        Assert.Equal(["List pets", "Create a pet", "Delete a pet"], pets.Children.Select(c => c.Name));
        Assert.Equal("{{baseUrl}}/pets", pets.Children[0].Request!.Url);
        Assert.Equal("GET", pets.Children[0].Request!.Method);
        Assert.Equal("{{baseUrl}}/pets/{{petId}}", pets.Children[2].Request!.Url);
        Assert.Equal("health", collection.Items.Single(i => i.Request is not null).Request!.SpecKey);
    }

    [Fact]
    public void A_json_body_is_built_from_the_schema_and_is_not_invented_for_get()
    {
        var pets = Folder(Import().Collection, "pets");

        Assert.Equal(string.Empty, pets.Children[0].Request!.Body);
        var body = pets.Children[1].Request!.Body;
        Assert.Contains("\"name\": \"\"", body);
        Assert.Contains("\"age\": 0", body);
        Assert.Contains("\"tags\": [", body);
    }

    [Fact]
    public void Auth_becomes_variables_and_no_value_from_the_description_is_copied()
    {
        var collection = Import().Collection;
        var pets = Folder(collection, "pets");

        var bearer = pets.Children[0].Request!.Auth!;
        Assert.Equal(AuthKinds.Bearer, bearer.Kind);
        Assert.Equal("{{bearerAuth}}", bearer.Token);
        Assert.True(collection.Variables.Single(v => v.Key == "bearerAuth").IsSecret);

        var apiKey = pets.Children[2].Request!.Auth!;
        Assert.Equal(AuthKinds.ApiKey, apiKey.Kind);
        Assert.Equal("X-Api-Key", apiKey.KeyName);
        Assert.Equal("{{apiKeyAuth}}", apiKey.KeyValue);

        // An operation that says no security needs none, even though the document has a global default.
        Assert.Null(collection.Items.Single(i => i.Request is not null).Request!.Auth);
        Assert.Equal("", collection.Variables.Single(v => v.Key == "bearerAuth").Value);
    }

    [Fact]
    public void A_rerun_keeps_what_the_user_added_and_reports_what_changed()
    {
        var first = Import().Collection;

        // The user adds a header, a body and a variable value on an existing request.
        var pets = Folder(first, "pets");
        var mine = pets.Children[1] with
        {
            Request = pets.Children[1].Request! with
            {
                Headers = [new KeyValue("X-Trace", "on")],
                Body = """{"name":"Rex"}""",
            },
        };
        var edited = first with
        {
            Items = first.Items.Select(i => i.Id == pets.Id ? pets with { Children = [pets.Children[0], mine, pets.Children[2]] } : i).ToList(),
            Variables = first.Variables.Select(v => v.Key == "baseUrl" ? v with { Value = "https://staging.example.test/v1" } : v).ToList(),
        };

        // The new description drops DELETE, renames one operation, and adds another.
        var changed = Spec
            .Replace("\"summary\": \"List pets\"", "\"summary\": \"List all pets\"")
            .Replace("\"/health\": {", "\"/owners\": { \"get\": { \"operationId\": \"listOwners\", \"summary\": \"Owners\", \"tags\": [\"owners\"] } },\n            \"/health\": {");
        changed = changed.Replace("\"delete\": { \"summary\": \"Delete a pet\", \"tags\": [\"pets\"], \"security\": [ { \"apiKeyAuth\": [] } ] }", "\"put\": { \"summary\": \"Put nothing\" }");

        var result = OpenApiImporter.Build(changed, Url, edited);
        var updatedPets = Folder(result.Collection, "pets");

        Assert.Equal("List all pets", updatedPets.Children[0].Name);
        var kept = updatedPets.Children[1].Request!;
        Assert.Equal([new KeyValue("X-Trace", "on")], kept.Headers);
        Assert.Equal("""{"name":"Rex"}""", kept.Body);

        // The collection keeps the staging URL the user set.
        Assert.Equal("https://staging.example.test/v1", result.Collection.Variables.Single(v => v.Key == "baseUrl").Value);

        Assert.Contains("Owners", Folder(result.Collection, "owners").Children.Select(c => c.Name));
        Assert.Contains("Delete a pet", result.Summary.Removed);
        Assert.Contains("owners / Owners", result.Summary.Added);
        Assert.Contains("List all pets", result.Summary.Changed);
        Assert.Equal(first.Id, result.Collection.Id);
        Assert.False(result.Summary.IsNew);
    }

    [Fact]
    public void Rerunning_an_unchanged_description_changes_nothing()
    {
        var first = Import().Collection;
        var again = OpenApiImporter.Build(Spec, Url, first).Summary;

        Assert.Empty(again.Added);
        Assert.Empty(again.Changed);
        Assert.Empty(again.Removed);
    }

    [Fact]
    public void Query_and_header_parameters_are_reported_rather_than_guessed()
    {
        var summary = Import().Summary;

        Assert.Contains(summary.Notes, n => n.Contains("query or header parameters"));
        Assert.True(summary.IsNew);
    }

    [Fact]
    public void A_relative_server_is_reported_and_the_description_is_still_read()
    {
        var json = Spec.Replace("https://api.example.test/v1/", "/v1");
        var summary = Import(json).Summary;

        Assert.Contains(summary.Notes, n => n.Contains("relative"));
    }

    [Fact]
    public void A_reference_outside_the_document_is_not_followed_and_is_reported()
    {
        var json = Spec.Replace("{ \"$ref\": \"#/components/schemas/Pet\" }", "{ \"$ref\": \"https://elsewhere.test/pet.json\" }");
        var result = Import(json);

        Assert.Contains(result.Summary.Notes, n => n.Contains("outside this description"));
        Assert.DoesNotContain("\"name\"", Folder(result.Collection, "pets").Children[1].Request!.Body);
    }

    [Theory]
    [InlineData("not json at all", "not JSON")]
    [InlineData("""{"swagger": "2.0"}""", "not an OpenAPI 3")]
    public void Anything_else_is_refused_with_a_clear_message(string json, string expected)
    {
        var error = Assert.Throws<OpenApiException>(() => Import(json));

        Assert.Contains(expected, error.Message);
    }

    [Fact]
    public async Task The_fetcher_refuses_other_schemes_and_returns_the_body()
    {
        var client = new HttpClient(new StubHandler(Spec));

        await Assert.ThrowsAsync<OpenApiException>(() => OpenApiFetcher.FetchAsync("file:///etc/passwd", client, TestContext.Current.CancellationToken));

        var body = await OpenApiFetcher.FetchAsync(Url, client, TestContext.Current.CancellationToken);
        Assert.Equal(Spec, body);
    }

    [Fact]
    public async Task The_fetcher_reports_a_server_error_without_throwing_anything_else()
    {
        var client = new HttpClient(new StubHandler(null, HttpStatusCode.NotFound));

        var error = await Assert.ThrowsAsync<OpenApiException>(() => OpenApiFetcher.FetchAsync(Url, client, TestContext.Current.CancellationToken));

        Assert.Contains("404", error.Message);
    }

    private sealed class StubHandler(string? body, HttpStatusCode status = HttpStatusCode.OK) : HttpMessageHandler
    {
        protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken) =>
            Task.FromResult(new HttpResponseMessage(status)
            {
                Content = new StringContent(body ?? "", Encoding.UTF8, "application/json"),
            });
    }
}
