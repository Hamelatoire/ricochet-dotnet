using System.Text.Json;
using Ricochet.Core;

namespace Ricochet.Core.Tests;

public class PostmanExporterTests
{
    private static Collection Sample() => new()
    {
        Name = "Shop",
        Variables =
        [
            new KeyValue("base", "https://shop.example.test"),
            new KeyValue("debug", "1", Enabled: false),
        ],
        Scripts = [new StoredScript("prerequest", "console.log('start');")],
        Items =
        [
            new CollectionItem
            {
                Name = "Products",
                Children =
                [
                    new CollectionItem
                    {
                        Name = "List",
                        Request = new ApiRequest
                        {
                            Name = "List",
                            Method = "GET",
                            Url = "{{base}}/products?limit=10",
                            Headers = [new KeyValue("Accept", "application/json"), new KeyValue("X-Trace", "off", Enabled: false)],
                            Auth = new RequestAuth { Kind = AuthKinds.Bearer, Token = "{{token}}" },
                        },
                        Scripts = [new StoredScript("test", "pm.test('ok', () => {});")],
                    },
                    new CollectionItem
                    {
                        Name = "Create",
                        Request = new ApiRequest
                        {
                            Name = "Create",
                            Method = "POST",
                            Url = "{{base}}/products",
                            Headers = [new KeyValue("Content-Type", "application/json")],
                            Body = """{ "name": "Tea", "price": 3 }""",
                            Auth = new RequestAuth { Kind = AuthKinds.ApiKey, KeyName = "X-Key", KeyValue = "{{apiKey}}", KeyInQuery = true },
                        },
                    },
                ],
            },
            new CollectionItem
            {
                Name = "Login",
                Request = new ApiRequest
                {
                    Name = "Login",
                    Method = "POST",
                    Url = "{{base}}/login",
                    Headers = [new KeyValue("Content-Type", "application/x-www-form-urlencoded")],
                    Body = "user=a&pass=b",
                    Auth = new RequestAuth { Kind = AuthKinds.Basic, Username = "{{user}}", Password = "{{pass}}" },
                },
            },
            new CollectionItem
            {
                Name = "Health",
                Request = new ApiRequest { Name = "Health", Method = "GET", Url = "http://health.example.test/", Auth = null },
            },
        ],
    };

    // The parts of each Request that a round trip must keep, with folders as part of the path.
    private static List<string> Flatten(IEnumerable<CollectionItem> items, string parent = "")
    {
        var lines = new List<string>();
        foreach (var item in items)
        {
            var path = parent.Length == 0 ? item.Name : $"{parent} / {item.Name}";
            if (item.Request is { } request)
            {
                var headers = string.Join(";", request.Headers.Select(h => $"{h.Key}={h.Value}:{h.Enabled}"));
                var script = string.Join(";", item.Scripts.Select(s => $"{s.Listen}={s.Source}"));
                lines.Add(string.Join("|", path, request.Method, request.Url, headers, request.Body, AuthText(request.Auth), script));
            }
            else
            {
                lines.AddRange(Flatten(item.Children, path));
                if (item.Scripts.Count > 0)
                    lines.Add($"{path} scripts={string.Join(";", item.Scripts.Select(s => s.Source))}");
            }
        }

        return lines;
    }

    // Auth that is missing and auth with kind none mean the same thing, so they compare equal.
    private static string AuthText(RequestAuth? auth) =>
        auth is null or { Kind: AuthKinds.None }
            ? "none"
            : string.Join("|", auth.Kind, auth.Token, auth.Username, auth.Password, auth.KeyName, auth.KeyValue, auth.KeyInQuery);

    private static JsonDocument Parse(string json) => JsonDocument.Parse(json);

    [Fact]
    public void Writes_a_v21_collection_with_folders_and_requests()
    {
        var result = PostmanExporter.Export(Sample());
        using var document = Parse(result.Json);
        var root = document.RootElement;

        Assert.Equal(PostmanExporter.Schema, root.GetProperty("info").GetProperty("schema").GetString());
        Assert.Equal("Shop", root.GetProperty("info").GetProperty("name").GetString());
        var products = root.GetProperty("item")[0];
        Assert.Equal("Products", products.GetProperty("name").GetString());
        Assert.Equal("GET", products.GetProperty("item")[0].GetProperty("request").GetProperty("method").GetString());
        Assert.DoesNotContain(result.Notes, n => n.Category == ExportCategories.SecretVariables);
    }

    [Fact]
    public void Postman_export_imports_back_to_the_same_requests()
    {
        var collection = Sample();
        var result = PostmanExporter.Export(collection);

        var imported = PostmanImporter.Import(result.Json, "shop.json").Collection!;

        Assert.Equal(Flatten(collection.Items), Flatten(imported.Items));
        Assert.Equal(collection.Name, imported.Name);
        Assert.Equal(
            collection.Variables.Select(v => (v.Key, v.Value, v.Enabled)),
            imported.Variables.Select(v => (v.Key, v.Value, v.Enabled)));
        Assert.Equal(collection.Scripts, imported.Scripts);
    }

    [Fact]
    public void A_secret_variable_is_left_out_and_its_references_stay_as_placeholders()
    {
        var collection = Sample() with
        {
            Variables = [new KeyValue("apiToken", "s3cr3t-value", IsSecret: true), new KeyValue("copy", "s3cr3t-value")],
            Items =
            [
                new CollectionItem
                {
                    Name = "Ping",
                    Request = new ApiRequest
                    {
                        Name = "Ping",
                        Method = "GET",
                        Url = "{{base}}/ping",
                        Headers = [new KeyValue("X-Token", "{{apiToken}}")],
                    },
                },
            ],
        };

        var result = PostmanExporter.Export(collection);

        Assert.DoesNotContain("s3cr3t-value", result.Json);
        using var document = Parse(result.Json);
        var variables = document.RootElement.GetProperty("variable").EnumerateArray()
            .Select(v => (Key: v.GetProperty("key").GetString(), Value: v.GetProperty("value").GetString()))
            .ToList();
        Assert.DoesNotContain(variables, v => v.Key == "apiToken");
        Assert.Contains(variables, v => v.Key == "copy" && v.Value == "{{apiToken}}");
        Assert.Contains(result.Notes, n => n.Category == ExportCategories.SecretVariables && n.Detail == "apiToken");
        Assert.Contains(result.Notes, n => n.Category == ExportCategories.SecretValues && n.Detail.Contains("{{apiToken}}"));
        Assert.Contains("{{apiToken}}", result.Json);
    }

    [Fact]
    public void Typed_in_secrets_are_replaced_with_placeholders_and_reported()
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
                        Url = "{{base}}/secure",
                        Headers = [new KeyValue("Authorization", "Bearer abc123-literal")],
                        Auth = new RequestAuth { Kind = AuthKinds.Bearer, Token = "tok-literal-999" },
                    },
                },
                new CollectionItem
                {
                    Name = "Basic",
                    Request = new ApiRequest
                    {
                        Name = "Basic",
                        Method = "GET",
                        Url = "{{base}}/basic",
                        Auth = new RequestAuth { Kind = AuthKinds.Basic, Username = "ann", Password = "pw-literal-42" },
                    },
                },
            ],
        };

        var result = PostmanExporter.Export(collection);

        Assert.DoesNotContain("abc123-literal", result.Json);
        Assert.DoesNotContain("tok-literal-999", result.Json);
        Assert.DoesNotContain("pw-literal-42", result.Json);
        Assert.Contains("{{Authorization}}", result.Json);
        Assert.Contains("{{token}}", result.Json);
        Assert.Contains("{{password}}", result.Json);
        Assert.Contains(result.Notes, n => n.Category == ExportCategories.SecretValues && n.Location.EndsWith("Secure"));
        Assert.Contains(result.Notes, n => n.Category == ExportCategories.SecretValues && n.Location.EndsWith("Basic"));
    }

    [Fact]
    public void Secret_values_are_not_written_into_a_script_or_body()
    {
        var collection = Sample() with
        {
            Variables = [new KeyValue("apiToken", "s3cr3t-value", IsSecret: true)],
            Scripts = [new StoredScript("prerequest", "const t = 's3cr3t-value';")],
            Items =
            [
                new CollectionItem
                {
                    Name = "Send",
                    Request = new ApiRequest { Name = "Send", Method = "POST", Url = "{{base}}/x", Body = "token=s3cr3t-value" },
                    Scripts = [new StoredScript("test", "s3cr3t-value")],
                },
            ],
        };

        var result = PostmanExporter.Export(collection);

        Assert.DoesNotContain("s3cr3t-value", result.Json);
        Assert.Contains("token={{apiToken}}", result.Json);
    }

    [Fact]
    public void Notes_say_what_postman_could_not_hold()
    {
        var collection = Sample() with
        {
            Items =
            [
                new CollectionItem
                {
                    Name = "Checked",
                    Request = new ApiRequest
                    {
                        Name = "Checked",
                        Method = "GET",
                        Url = "{{base}}/checked",
                        Assertions = [new RequestAssertion { Expected = "200" }],
                    },
                },
            ],
        };

        var result = PostmanExporter.Export(collection);

        Assert.Contains(result.Notes, n => n.Category == ExportCategories.NotExported && n.Location.EndsWith("Checked") && n.Detail.Contains("assertion"));
        Assert.Contains("Not exported", result.ToReportText());
    }
}
