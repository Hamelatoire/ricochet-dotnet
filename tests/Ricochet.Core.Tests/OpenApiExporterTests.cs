using System.Text.Json;
using Ricochet.Core;

namespace Ricochet.Core.Tests;

public class OpenApiExporterTests
{
    private static Collection Sample() => new()
    {
        Name = "Shop",
        Variables = [new KeyValue("apiToken", "s3cr3t-value", IsSecret: true)],
        Items =
        [
            new CollectionItem
            {
                Name = "Products",
                Children =
                [
                    new CollectionItem
                    {
                        Name = "Get product",
                        Request = new ApiRequest
                        {
                            Name = "Get product",
                            Method = "GET",
                            Url = "https://shop.example.test/products/{{productId}}?limit=10",
                            Headers = [new KeyValue("Accept", "application/json"), new KeyValue("X-Trace", "off", Enabled: false)],
                            Auth = new RequestAuth { Kind = AuthKinds.Bearer, Token = "{{token}}" },
                        },
                    },
                    new CollectionItem
                    {
                        Name = "Create product",
                        Request = new ApiRequest
                        {
                            Name = "Create product",
                            Method = "POST",
                            Url = "https://shop.example.test/products",
                            Headers = [new KeyValue("Content-Type", "application/json")],
                            Body = """{ "name": "Tea" }""",
                            Auth = new RequestAuth { Kind = AuthKinds.Bearer, Token = "{{token}}" },
                        },
                    },
                ],
            },
        ],
    };

    private static JsonDocument Parse(string json) => JsonDocument.Parse(json);

    [Fact]
    public void Writes_an_openapi_3_document_with_one_operation_per_request()
    {
        var result = OpenApiExporter.Export(Sample());
        using var document = Parse(result.Json);
        var root = document.RootElement;

        Assert.Equal("3.0.3", root.GetProperty("openapi").GetString());
        Assert.Equal("Shop", root.GetProperty("info").GetProperty("title").GetString());
        Assert.Equal("https://shop.example.test", root.GetProperty("servers")[0].GetProperty("url").GetString());

        var get = root.GetProperty("paths").GetProperty("/products/{productId}").GetProperty("get");
        Assert.Equal("getproduct", get.GetProperty("operationId").GetString());
        Assert.Equal("Products", get.GetProperty("tags")[0].GetString());
        Assert.Equal(["bearerAuth"], get.GetProperty("security")[0].EnumerateObject().Select(p => p.Name));

        var parameters = get.GetProperty("parameters").EnumerateArray()
            .Select(p => (Name: p.GetProperty("name").GetString(), In: p.GetProperty("in").GetString(), Required: p.GetProperty("required").GetBoolean()))
            .ToList();
        Assert.Contains(("productId", "path", true), parameters);
        Assert.Contains(("limit", "query", false), parameters);
        Assert.Contains(("Accept", "header", false), parameters);
        Assert.DoesNotContain(parameters, p => p.Name == "X-Trace");

        var post = root.GetProperty("paths").GetProperty("/products").GetProperty("post");
        var body = post.GetProperty("requestBody").GetProperty("content").GetProperty("application/json");
        Assert.Equal("""{ "name": "Tea" }""", body.GetProperty("example").GetString());

        var scheme = root.GetProperty("components").GetProperty("securitySchemes").GetProperty("bearerAuth");
        Assert.Equal("http", scheme.GetProperty("type").GetString());
        Assert.Equal("bearer", scheme.GetProperty("scheme").GetString());
    }

    [Fact]
    public void No_secret_value_or_sensitive_header_appears_in_the_document()
    {
        var collection = Sample() with
        {
            Items =
            [
                new CollectionItem
                {
                    Name = "Secure",
                    Request = new ApiRequest
                    {
                        Name = "Secure",
                        Method = "GET",
                        Url = "https://shop.example.test/secure?key={{apiToken}}",
                        Headers = [new KeyValue("Authorization", "Bearer abc123-literal"), new KeyValue("X-Token", "{{apiToken}}")],
                        Auth = new RequestAuth { Kind = AuthKinds.Basic, Username = "ann", Password = "pw-literal-42" },
                    },
                },
            ],
        };

        var result = OpenApiExporter.Export(collection);

        Assert.DoesNotContain("s3cr3t-value", result.Json);
        Assert.DoesNotContain("abc123-literal", result.Json);
        Assert.DoesNotContain("pw-literal-42", result.Json);
        Assert.DoesNotContain("\"Authorization\"", result.Json);
        Assert.Contains(result.Notes, n => n.Category == ExportCategories.SecretVariables && n.Detail == "apiToken");
        Assert.Contains(result.Notes, n => n.Category == ExportCategories.SecretValues && n.Location.EndsWith("Secure"));
    }

    [Fact]
    public void A_base_address_held_in_a_variable_is_reported_and_the_server_set_by_hand()
    {
        var collection = Sample() with
        {
            Items =
            [
                new CollectionItem
                {
                    Name = "Relative",
                    Request = new ApiRequest { Name = "Relative", Method = "GET", Url = "{{baseUrl}}/items" },
                },
            ],
        };

        var result = OpenApiExporter.Export(collection);

        using var document = Parse(result.Json);
        Assert.Equal(OpenApiExporterDefaultServer, document.RootElement.GetProperty("servers")[0].GetProperty("url").GetString());
        Assert.Contains(result.Notes, n => n.Detail.Contains("{{baseUrl}}", StringComparison.Ordinal));
        Assert.True(document.RootElement.GetProperty("paths").TryGetProperty("/items", out _));
    }

    [Fact]
    public void Scripts_assertions_and_unsupported_methods_are_reported()
    {
        var collection = Sample() with
        {
            Items =
            [
                new CollectionItem
                {
                    Name = "Odd",
                    Request = new ApiRequest
                    {
                        Name = "Odd",
                        Method = "PROPFIND",
                        Url = "https://shop.example.test/odd",
                        Assertions = [new RequestAssertion { Expected = "200" }],
                    },
                    Scripts = [new StoredScript("test", "pm.test('x', () => {});")],
                },
            ],
        };

        var result = OpenApiExporter.Export(collection);

        Assert.Contains(result.Notes, n => n.Detail.Contains("PROPFIND", StringComparison.Ordinal));
        Assert.Contains(result.Notes, n => n.Detail.Contains("script", StringComparison.Ordinal));
        using var document = Parse(result.Json);
        Assert.False(document.RootElement.GetProperty("paths").TryGetProperty("/odd", out _));
    }

    [Fact]
    public void The_openapi_importer_reads_the_export_back()
    {
        var result = OpenApiExporter.Export(Sample());

        var imported = OpenApiImporter.Build(result.Json, "https://shop.example.test/openapi.json", null).Collection;

        var requests = Requests(imported.Items).ToList();
        Assert.Equal(2, requests.Count);
        Assert.Contains(requests, r => r.Method == "GET" && r.Url == "{{baseUrl}}/products/{{productId}}");
        Assert.Contains(requests, r => r.Method == "POST" && r.Url == "{{baseUrl}}/products");
    }

    private const string OpenApiExporterDefaultServer = "http://localhost";

    private static IEnumerable<ApiRequest> Requests(IEnumerable<CollectionItem> items)
    {
        foreach (var item in items)
        {
            if (item.Request is { } request)
                yield return request;
            foreach (var child in Requests(item.Children))
                yield return child;
        }
    }
}
