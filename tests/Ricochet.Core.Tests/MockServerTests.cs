using System.Net;
using System.Net.Sockets;
using System.Text;
using Ricochet.Core;

namespace Ricochet.Core.Tests;

public class MockServerTests
{
    private const string Spec = """
        {
          "openapi": "3.0.3",
          "info": { "title": "Pets" },
          "servers": [ { "url": "https://api.example.test/v1" } ],
          "paths": {
            "/pets": {
              "get": { "responses": { "200": { "description": "ok", "content": { "application/json": {
                "example": [ { "id": 1, "name": "Rex" } ] } } } } },
              "post": { "responses": { "201": { "description": "made", "content": { "application/json": {
                "schema": { "$ref": "#/components/schemas/Pet" } } } } } }
            },
            "/pets/{petId}": {
              "get": { "responses": { "200": { "description": "one", "content": { "application/json": {
                "examples": { "rex": { "value": { "id": 1, "name": "Rex" } } } } } } } },
              "delete": { "responses": { "204": { "description": "gone" } } }
            },
            "/pets/search": {
              "get": { "responses": { "default": { "description": "anything", "content": { "text/plain": {
                "example": "found" } } } } }
            }
          },
          "components": {
            "schemas": {
              "Pet": {
                "type": "object",
                "properties": {
                  "name": { "type": "string" },
                  "age": { "type": "integer" },
                  "active": { "type": "boolean" },
                  "tags": { "type": "array", "items": { "type": "string" } },
                  "owner": { "$ref": "#/components/schemas/Owner" },
                  "status": { "type": "string", "enum": ["sold", "available"] },
                  "born": { "type": "string", "format": "date" }
                }
              },
              "Owner": {
                "allOf": [
                  { "$ref": "#/components/schemas/Named" },
                  { "type": "object", "properties": { "email": { "type": "string", "format": "email" } } }
                ]
              },
              "Named": { "type": "object", "properties": { "fullName": { "type": "string" } } }
            }
          }
        }
        """;

    private static string Body(MockReply reply) => Encoding.UTF8.GetString(reply.Body);

    private static MockRouteTable Table(params MockRoute[] routes) => new(routes);

    private static MockRoute Route(string method, string path, int status = 200, string body = "") =>
        new(method, path, new MockReply(status, [], Encoding.UTF8.GetBytes(body), "text/plain"));

    [Fact]
    public void Literal_path_matches_only_the_same_method_and_segments()
    {
        var table = Table(Route("GET", "/pets"));

        Assert.NotNull(table.Find("get", "/pets"));
        Assert.Null(table.Find("GET", "/pets/1"));
        Assert.Null(table.Find("GET", "/pet"));
    }

    [Fact]
    public void Query_string_and_trailing_slash_are_ignored()
    {
        var table = Table(Route("GET", "/pets"));

        Assert.NotNull(table.Find("GET", "/pets?limit=2&key=secret"));
        Assert.NotNull(table.Find("GET", "/pets/"));
        Assert.NotNull(table.Find("GET", "http://127.0.0.1:8080/pets#top"));
    }

    [Fact]
    public void Parameter_segments_match_any_one_segment_in_all_three_styles()
    {
        foreach (var template in new[] { "/pets/{id}", "/pets/{{id}}", "/pets/:id" })
        {
            var table = Table(Route("GET", template));
            Assert.NotNull(table.Find("GET", "/pets/42"));
            Assert.Null(table.Find("GET", "/pets/42/toys"));
            Assert.Null(table.Find("GET", "/pets"));
        }
    }

    [Fact]
    public void A_literal_route_beats_a_parameter_route_on_the_same_path()
    {
        var table = Table(
            Route("GET", "/pets/{id}", body: "by id"),
            Route("GET", "/pets/search", body: "search"));

        Assert.Equal("search", Body(table.Respond("GET", "/pets/search")));
        Assert.Equal("by id", Body(table.Respond("GET", "/pets/7")));
    }

    [Fact]
    public void Where_two_routes_tie_the_first_registered_wins()
    {
        var table = Table(Route("GET", "/a/{x}", body: "first"), Route("GET", "/a/{y}", body: "second"));

        Assert.Equal("first", Body(table.Respond("GET", "/a/1")));
    }

    [Fact]
    public void An_unknown_path_gets_a_404_as_json()
    {
        var reply = Table(Route("GET", "/pets")).Respond("GET", "/owners?key=abc");

        Assert.Equal(404, reply.StatusCode);
        Assert.Equal("application/json; charset=utf-8", reply.MediaType);
        Assert.Contains("No mock matches GET /owners.", Body(reply));
        // The query string is not echoed back, since it can hold a key.
        Assert.DoesNotContain("abc", Body(reply));
    }

    [Fact]
    public void A_known_path_with_another_method_gets_a_405_listing_the_allowed_methods()
    {
        var table = Table(Route("GET", "/pets/{id}"), Route("DELETE", "/pets/{id}"));

        var reply = table.Respond("PATCH", "/pets/3");

        Assert.Equal(405, reply.StatusCode);
        Assert.Contains(reply.Headers, h => h.Key == "Allow" && h.Value == "DELETE, GET");
    }

    [Fact]
    public void Examples_become_routes_keyed_on_the_path_of_their_url()
    {
        var routes = MockRouteBuilder.FromExamples(
        [
            new MockExample("get", "{{baseUrl}}/pets/{{petId}}?verbose=1", 200, "application/json", "{\"id\":1}"),
            new MockExample("GET", "https://api.example.test/pets/9", 200, "application/json", "{\"id\":9}"),
            new MockExample("GET", "/pets/{{petId}}", 500, "text/plain", "second, skipped"),
        ]);

        var table = new MockRouteTable(routes);
        Assert.Equal(2, routes.Count);
        // Only the first example for GET /pets/{id} is kept, so the 500 one is never sent.
        Assert.Equal("{\"id\":1}", Body(table.Respond("GET", "/pets/5")));
        Assert.Equal(200, table.Respond("GET", "/pets/5").StatusCode);
        Assert.Equal("{\"id\":9}", Body(table.Respond("GET", "/pets/9")));
    }

    [Fact]
    public void A_description_produces_one_route_per_operation_under_the_server_path()
    {
        var routes = MockRouteBuilder.FromOpenApi(Spec);
        var table = new MockRouteTable(routes);

        Assert.Equal(5, routes.Count);
        Assert.NotNull(table.Find("GET", "/v1/pets"));
        Assert.NotNull(table.Find("POST", "/v1/pets"));
        Assert.NotNull(table.Find("DELETE", "/v1/pets/3"));
        Assert.Null(table.Find("GET", "/pets"));
    }

    [Fact]
    public void A_response_uses_the_media_type_example_then_examples_then_a_schema_sample()
    {
        var table = new MockRouteTable(MockRouteBuilder.FromOpenApi(Spec));

        Assert.Equal("[{\"id\":1,\"name\":\"Rex\"}]", Body(table.Respond("GET", "/v1/pets")));
        Assert.Equal("{\"id\":1,\"name\":\"Rex\"}", Body(table.Respond("GET", "/v1/pets/3")));
    }

    [Fact]
    public void A_schema_sample_follows_references_allOf_enums_and_formats()
    {
        var reply = new MockRouteTable(MockRouteBuilder.FromOpenApi(Spec)).Respond("POST", "/v1/pets");

        Assert.Equal(201, reply.StatusCode);
        Assert.Equal("application/json", reply.MediaType);
        var json = Body(reply);
        Assert.Contains("\"name\":\"string\"", json);
        Assert.Contains("\"age\":0", json);
        Assert.Contains("\"active\":true", json);
        Assert.Contains("\"tags\":[\"string\"]", json);
        Assert.Contains("\"status\":\"sold\"", json);
        Assert.Contains("\"born\":\"2026-01-01\"", json);
        Assert.Contains("\"owner\":{\"fullName\":\"string\",\"email\":\"user@example.com\"}", json);
    }

    [Fact]
    public void A_204_has_an_empty_body_and_no_media_type()
    {
        var reply = new MockRouteTable(MockRouteBuilder.FromOpenApi(Spec)).Respond("DELETE", "/v1/pets/3");

        Assert.Equal(204, reply.StatusCode);
        Assert.Empty(reply.Body);
        Assert.Null(reply.MediaType);
    }

    [Fact]
    public void A_non_json_example_is_sent_as_text_and_the_default_response_answers_as_200()
    {
        var reply = new MockRouteTable(MockRouteBuilder.FromOpenApi(Spec)).Respond("GET", "/v1/pets/search");

        Assert.Equal(200, reply.StatusCode);
        Assert.Equal("text/plain", reply.MediaType);
        Assert.Equal("found", Body(reply));
    }

    [Fact]
    public void Text_that_is_not_OpenAPI_3_is_refused_with_a_message_for_the_user()
    {
        var notJson = Assert.Throws<MockServerException>(() => MockRouteBuilder.FromOpenApi("openapi: 3.0.0"));
        Assert.Contains("not JSON", notJson.Message);

        var swagger2 = Assert.Throws<MockServerException>(() => MockRouteBuilder.FromOpenApi("{\"swagger\":\"2.0\",\"paths\":{}}"));
        Assert.Equal("This is not an OpenAPI 3 description.", swagger2.Message);

        var noPaths = Assert.Throws<MockServerException>(() => MockRouteBuilder.FromOpenApi("{\"openapi\":\"3.0.0\"}"));
        Assert.Contains("no paths", noPaths.Message);
    }

    [Fact]
    public async Task The_server_answers_over_loopback_and_logs_the_path_without_the_query()
    {
        var port = FreePort();
        var server = new MockServer();
        var logged = new List<MockLogEntry>();
        var seen = new SemaphoreSlim(0);
        server.RequestHandled += entry =>
        {
            lock (logged) logged.Add(entry);
            seen.Release();
        };

        server.Start(port, Table(Route("GET", "/pets/{id}", 200, "{\"ok\":true}")));
        try
        {
            using var client = new HttpClient();
            var hit = await client.GetAsync($"http://127.0.0.1:{port}/pets/4?key=secret", TestContext.Current.CancellationToken);
            var miss = await client.GetAsync($"http://127.0.0.1:{port}/nope", TestContext.Current.CancellationToken);

            Assert.Equal(HttpStatusCode.OK, hit.StatusCode);
            Assert.Equal("{\"ok\":true}", await hit.Content.ReadAsStringAsync(TestContext.Current.CancellationToken));
            Assert.Equal(HttpStatusCode.NotFound, miss.StatusCode);
            Assert.True(await seen.WaitAsync(TimeSpan.FromSeconds(5), TestContext.Current.CancellationToken));
            Assert.True(await seen.WaitAsync(TimeSpan.FromSeconds(5), TestContext.Current.CancellationToken));

            lock (logged)
            {
                Assert.Equal(["/pets/4", "/nope"], logged.Select(e => e.Path));
                Assert.Equal([200, 404], logged.Select(e => e.StatusCode));
                Assert.DoesNotContain(logged, e => e.Path.Contains("secret"));
            }
        }
        finally
        {
            server.Stop();
        }

        Assert.False(server.IsRunning);
    }

    [Fact]
    public void The_server_refuses_ports_outside_the_user_range_and_a_second_start()
    {
        var server = new MockServer();
        Assert.Throws<MockServerException>(() => server.Start(80, Table()));

        var port = FreePort();
        server.Start(port, Table());
        try
        {
            Assert.Throws<MockServerException>(() => server.Start(FreePort(), Table()));
        }
        finally
        {
            server.Stop();
        }
    }

    private static int FreePort()
    {
        var probe = new TcpListener(IPAddress.Loopback, 0);
        probe.Start();
        var port = ((IPEndPoint)probe.LocalEndpoint).Port;
        probe.Stop();
        return port;
    }
}
