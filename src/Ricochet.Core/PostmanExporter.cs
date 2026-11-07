using System.Text.Encodings.Web;
using System.Text.Json;
using System.Text.Json.Nodes;

namespace Ricochet.Core;

/// <summary>
/// Writes a Collection as a Postman v2.1 collection that <see cref="PostmanImporter"/> reads back (ADR 0029).
/// Secret values become {{placeholders}} and secret Variables are left out. See <see cref="ExportResult"/> for the notes.
/// </summary>
public static class PostmanExporter
{
    public const string Schema = "https://schema.getpostman.com/json/collection/v2.1.0/collection.json";

    public static ExportResult Export(Collection collection)
    {
        var context = new ExportContext(collection);
        var root = new JsonObject
        {
            ["info"] = new JsonObject
            {
                ["_postman_id"] = collection.Id,
                ["name"] = collection.Name,
                ["schema"] = Schema,
            },
            ["item"] = Items(collection.Items, context, collection.Name),
        };

        var variables = new JsonArray();
        foreach (var variable in collection.Variables.Where(v => !v.IsSecret))
        {
            var entry = new JsonObject
            {
                ["key"] = variable.Key,
                ["value"] = context.Scrub(variable.Value, $"{collection.Name} variable {variable.Key}"),
            };
            if (!variable.Enabled)
                entry["disabled"] = true;
            JsonHelpers.Append(variables, entry);
        }

        if (variables.Count > 0)
            root["variable"] = variables;

        var events = Events(collection.Scripts, context, collection.Name);
        if (events.Count > 0)
            root["event"] = events;

        return new ExportResult(Serialize(root), context.Notes);
    }

    private static JsonArray Items(IEnumerable<CollectionItem> items, ExportContext context, string location)
    {
        var array = new JsonArray();
        foreach (var item in items)
        {
            var path = $"{location} / {item.Name}";
            var node = new JsonObject { ["name"] = item.Name };
            if (item.Request is { } request)
                node["request"] = Request(request, context, path);
            else
                node["item"] = Items(item.Children, context, path);

            var events = Events(item.Scripts, context, path);
            if (events.Count > 0)
                node["event"] = events;

            JsonHelpers.Append(array, node);
        }

        return array;
    }

    private static JsonObject Request(ApiRequest request, ExportContext context, string location)
    {
        var prepared = PreparedRequest.Create(context, request, location);
        foreach (var typed in prepared.TypedSecrets)
        {
            context.Notes.Add(new ExportNote(ExportCategories.SecretValues, location,
                $"{typed.Field} was typed in as a value, so {typed.Placeholder} is written instead. Set it as a Variable."));
        }

        if (request.Assertions is { Count: > 0 } assertions)
            context.Notes.Add(new ExportNote(ExportCategories.NotExported, location, $"{ExportContext.Plural(assertions.Count, "assertion")} (Postman tests are not written)"));
        if (request.Captures is { Count: > 0 } captures)
            context.Notes.Add(new ExportNote(ExportCategories.NotExported, location, $"{ExportContext.Plural(captures.Count, "capture")}"));

        var node = new JsonObject
        {
            ["method"] = prepared.Method,
            ["header"] = Headers(prepared.Headers),
            ["url"] = new JsonObject { ["raw"] = prepared.Url },
            ["auth"] = Auth(prepared.Auth),
        };

        if (prepared.Body.Length > 0)
        {
            node["body"] = new JsonObject
            {
                ["mode"] = "raw",
                ["raw"] = prepared.Body,
                ["options"] = new JsonObject
                {
                    ["raw"] = new JsonObject { ["language"] = Language(prepared.Headers) },
                },
            };
        }

        return node;
    }

    private static JsonArray Headers(IEnumerable<KeyValue> headers)
    {
        var array = new JsonArray();
        foreach (var header in headers)
        {
            var entry = new JsonObject { ["key"] = header.Key, ["value"] = header.Value };
            if (!header.Enabled)
                entry["disabled"] = true;
            JsonHelpers.Append(array, entry);
        }

        return array;
    }

    private static JsonObject Auth(RequestAuth auth) => auth.Kind switch
    {
        AuthKinds.Bearer => new JsonObject
        {
            ["type"] = "bearer",
            ["bearer"] = Pairs(("token", auth.Token)),
        },
        AuthKinds.Basic => new JsonObject
        {
            ["type"] = "basic",
            ["basic"] = Pairs(("username", auth.Username), ("password", auth.Password)),
        },
        AuthKinds.ApiKey => new JsonObject
        {
            ["type"] = "apikey",
            ["apikey"] = Pairs(("key", auth.KeyName), ("value", auth.KeyValue), ("in", auth.KeyInQuery ? "query" : "header")),
        },
        _ => new JsonObject { ["type"] = "noauth" },
    };

    private static JsonArray Pairs(params (string Key, string Value)[] pairs)
    {
        var array = new JsonArray();
        foreach (var (key, value) in pairs)
            JsonHelpers.Append(array, new JsonObject { ["key"] = key, ["value"] = value, ["type"] = "string" });
        return array;
    }

    private static string Language(IReadOnlyList<KeyValue> headers)
    {
        var type = PreparedRequest.ContentType(headers) ?? "";
        if (type.Contains("json", StringComparison.OrdinalIgnoreCase)) return "json";
        if (type.Contains("xml", StringComparison.OrdinalIgnoreCase)) return "xml";
        if (type.Contains("html", StringComparison.OrdinalIgnoreCase)) return "html";
        if (type.Contains("javascript", StringComparison.OrdinalIgnoreCase)) return "javascript";
        return "text";
    }

    private static JsonArray Events(IEnumerable<StoredScript> scripts, ExportContext context, string location)
    {
        var array = new JsonArray();
        foreach (var script in scripts)
        {
            var source = context.Scrub(script.Source, location);
            var lines = new JsonArray();
            foreach (var line in source.Split('\n'))
                JsonHelpers.Append(lines, line);

            JsonHelpers.Append(array, new JsonObject
            {
                ["listen"] = script.Listen,
                ["script"] = new JsonObject { ["type"] = "text/javascript", ["exec"] = lines },
            });
        }

        return array;
    }

    private static string Serialize(JsonNode node) => node.ToJsonString(new JsonSerializerOptions
    {
        WriteIndented = true,
        Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
    });
}
