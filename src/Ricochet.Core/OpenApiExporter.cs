using System.Text;
using System.Text.Encodings.Web;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.RegularExpressions;

namespace Ricochet.Core;

/// <summary>
/// Writes a Collection as an OpenAPI 3.0 document in JSON (ADR 0029): one operation per Request, grouped by Folder.
/// No secret value is written. Sign-in is described by its scheme only, and sensitive headers are left out.
/// </summary>
public static partial class OpenApiExporter
{
    private const string Version = "3.0.3";
    private const string DefaultServer = "http://localhost";
    private static readonly string[] Methods = ["get", "put", "post", "delete", "options", "head", "patch", "trace"];

    [GeneratedRegex(@"\{\{([^{}]+)\}\}")]
    private static partial Regex PlaceholderInPath();

    private sealed record ParsedUrl(string? Origin, string Path, IReadOnlyList<string> PathParams, IReadOnlyList<string> QueryKeys);

    private sealed record Pending(string Name, string Tag, string Location, PreparedRequest Prepared, ParsedUrl Url);

    public static ExportResult Export(Collection collection)
    {
        var context = new ExportContext(collection);
        var pending = new List<Pending>();
        Walk(collection.Items, collection.Name, tag: null, context, pending);

        if (collection.Variables.Any(v => !v.IsSecret))
        {
            var count = collection.Variables.Count(v => !v.IsSecret);
            context.Notes.Add(new ExportNote(ExportCategories.NotExported, collection.Name,
                $"{ExportContext.Plural(count, "variable")} (OpenAPI has no variables, so their values are not written)"));
        }

        var servers = Servers(pending, collection.Name, context);
        var paths = new JsonObject();
        var schemes = new JsonObject();
        var operationIds = new HashSet<string>(StringComparer.Ordinal);
        var seen = new HashSet<string>(StringComparer.Ordinal);

        foreach (var item in pending)
        {
            var method = item.Prepared.Method.ToLowerInvariant();
            if (Array.IndexOf(Methods, method) < 0)
            {
                context.Notes.Add(new ExportNote(ExportCategories.NotExported, item.Location, $"the method {item.Prepared.Method} has no OpenAPI equivalent"));
                continue;
            }

            if (!seen.Add($"{method} {item.Url.Path}"))
            {
                context.Notes.Add(new ExportNote(ExportCategories.NotExported, item.Location, $"the same method and path as an earlier request, which OpenAPI cannot hold twice"));
                continue;
            }

            paths[item.Url.Path] ??= new JsonObject();
            ((JsonObject)paths[item.Url.Path]!)[method] = Operation(item, operationIds, schemes);
        }

        var document = new JsonObject
        {
            ["openapi"] = Version,
            ["info"] = new JsonObject { ["title"] = collection.Name, ["version"] = "1.0.0" },
            ["servers"] = servers,
            ["paths"] = paths,
        };

        if (schemes.Count > 0)
            document["components"] = new JsonObject { ["securitySchemes"] = schemes };

        return new ExportResult(Serialize(document), context.Notes);
    }

    private static void Walk(IEnumerable<CollectionItem> items, string location, string? tag, ExportContext context, List<Pending> pending)
    {
        foreach (var item in items)
        {
            var path = $"{location} / {item.Name}";
            if (item.Scripts.Count > 0)
                context.Notes.Add(new ExportNote(ExportCategories.NotExported, path, $"{ExportContext.Plural(item.Scripts.Count, "script")} (OpenAPI has no place for scripts)"));

            if (item.Request is { } request)
            {
                if (request.Assertions is { Count: > 0 } assertions)
                    context.Notes.Add(new ExportNote(ExportCategories.NotExported, path, $"{ExportContext.Plural(assertions.Count, "assertion")} (OpenAPI has no tests)"));

                var prepared = PreparedRequest.Create(context, request, path);
                foreach (var typed in prepared.TypedSecrets)
                {
                    context.Notes.Add(new ExportNote(ExportCategories.SecretValues, path,
                        $"{typed.Field} was typed in as a value, so it is not written. The sign-in scheme still says what is needed."));
                }

                pending.Add(new Pending(item.Name, tag ?? "", path, prepared, ParseUrl(prepared.Url)));
            }
            else
            {
                Walk(item.Children, path, item.Name, context, pending);
            }
        }
    }

    private static JsonArray Servers(List<Pending> pending, string collectionName, ExportContext context)
    {
        var origins = pending.Select(p => p.Url.Origin).OfType<string>().Where(o => !o.Contains("{{", StringComparison.Ordinal)).Distinct().ToList();
        var placeholders = pending.Select(p => p.Url.Origin).OfType<string>().Where(o => o.Contains("{{", StringComparison.Ordinal)).Distinct().ToList();

        var servers = new JsonArray();
        foreach (var origin in origins)
            JsonHelpers.Append(servers, new JsonObject { ["url"] = origin });

        if (placeholders.Count > 0)
        {
            context.Notes.Add(new ExportNote(ExportCategories.NotExported, collectionName,
                $"requests start with {string.Join(", ", placeholders)}, which is not a server address, so set the server by hand"));
        }

        if (servers.Count == 0)
        {
            JsonHelpers.Append(servers, new JsonObject { ["url"] = DefaultServer, ["description"] = "Replace with the real server address." });
            context.Notes.Add(new ExportNote(ExportCategories.NotExported, collectionName,
                $"no request has a full address, so the server is set to {DefaultServer}"));
        }

        return servers;
    }

    private static JsonObject Operation(Pending item, HashSet<string> operationIds, JsonObject schemes)
    {
        var prepared = item.Prepared;
        var node = new JsonObject
        {
            ["operationId"] = UniqueId(item.Name, operationIds),
            ["summary"] = item.Name,
        };

        if (item.Tag.Length > 0)
            node["tags"] = new JsonArray(item.Tag);

        var parameters = new JsonArray();
        var added = new HashSet<string>(StringComparer.Ordinal);
        foreach (var name in item.Url.PathParams)
        {
            if (added.Add($"path {name}"))
                JsonHelpers.Append(parameters, Parameter(name, "path", required: true));
        }

        foreach (var name in item.Url.QueryKeys)
        {
            if (added.Add($"query {name}"))
                JsonHelpers.Append(parameters, Parameter(name, "query", required: false));
        }

        foreach (var header in prepared.Headers)
        {
            // Sensitive headers stay out entirely: their scheme is described below, and their values never appear.
            if (!header.Enabled || SecretRules.IsSensitiveHeader(header.Key) || header.Key.Equals("Content-Type", StringComparison.OrdinalIgnoreCase))
                continue;

            if (added.Add($"header {header.Key}"))
                JsonHelpers.Append(parameters, Parameter(header.Key, "header", required: false));
        }

        if (parameters.Count > 0)
            node["parameters"] = parameters;

        if (prepared.Body.Length > 0)
        {
            var mediaType = PreparedRequest.ContentType(prepared.Headers) is { } contentType && contentType.Split(';')[0].Trim() is { Length: > 0 } type
                ? type
                : prepared.Body.TrimStart().StartsWith('{') || prepared.Body.TrimStart().StartsWith('[') ? "application/json" : "text/plain";

            node["requestBody"] = new JsonObject
            {
                ["content"] = new JsonObject
                {
                    [mediaType] = new JsonObject
                    {
                        ["schema"] = new JsonObject { ["type"] = "string" },
                        ["example"] = prepared.Body,
                    },
                },
            };
        }

        if (SecurityScheme(prepared.Auth, schemes) is { } scheme)
            node["security"] = new JsonArray(new JsonObject { [scheme] = new JsonArray() });

        node["responses"] = new JsonObject
        {
            ["default"] = new JsonObject { ["description"] = "Response" },
        };

        return node;
    }

    private static JsonObject Parameter(string name, string location, bool required) => new()
    {
        ["name"] = name,
        ["in"] = location,
        ["required"] = required,
        ["schema"] = new JsonObject { ["type"] = "string" },
    };

    private static string? SecurityScheme(RequestAuth auth, JsonObject schemes)
    {
        string name;
        JsonObject scheme;
        switch (auth.Kind)
        {
            case AuthKinds.Bearer:
                name = "bearerAuth";
                scheme = new JsonObject { ["type"] = "http", ["scheme"] = "bearer" };
                break;
            case AuthKinds.Basic:
                name = "basicAuth";
                scheme = new JsonObject { ["type"] = "http", ["scheme"] = "basic" };
                break;
            case AuthKinds.ApiKey when auth.KeyName.Length > 0:
                var location = auth.KeyInQuery ? "query" : "header";
                name = $"apiKey_{PreparedRequest.Clean(auth.KeyName, "custom")}_{location}";
                scheme = new JsonObject { ["type"] = "apiKey", ["name"] = auth.KeyName, ["in"] = location };
                break;
            default:
                return null;
        }

        schemes[name] ??= scheme;
        return name;
    }

    private static string UniqueId(string name, HashSet<string> taken)
    {
        var words = new StringBuilder();
        foreach (var c in name.Where(char.IsAsciiLetterOrDigit))
            words.Append(words.Length == 0 ? char.ToLowerInvariant(c) : c);

        var baseId = words.Length > 0 ? words.ToString() : "request";
        var id = baseId;
        for (var n = 2; !taken.Add(id); n++)
            id = $"{baseId}{n}";
        return id;
    }

    private static ParsedUrl ParseUrl(string url)
    {
        var fragment = url.IndexOf('#', StringComparison.Ordinal);
        if (fragment >= 0)
            url = url[..fragment];

        var query = "";
        var question = url.IndexOf('?', StringComparison.Ordinal);
        if (question >= 0)
        {
            query = url[(question + 1)..];
            url = url[..question];
        }

        string? origin = null;
        var rest = url;
        if (url.StartsWith("{{", StringComparison.Ordinal))
        {
            var close = url.IndexOf("}}", StringComparison.Ordinal);
            if (close >= 0)
            {
                origin = url[..(close + 2)];
                rest = url[(close + 2)..];
            }
        }
        else if (url.IndexOf("://", StringComparison.Ordinal) is var scheme and >= 0)
        {
            var slash = url.IndexOf('/', scheme + 3);
            origin = slash < 0 ? url : url[..slash];
            rest = slash < 0 ? "" : url[slash..];
        }

        var pathParams = new List<string>();
        var path = PlaceholderInPath().Replace(rest, match =>
        {
            var name = PreparedRequest.Clean(match.Groups[1].Value, "param");
            pathParams.Add(name);
            return "{" + name + "}";
        });

        if (path.Length == 0)
            path = "/";
        else if (!path.StartsWith('/'))
            path = "/" + path;

        var queryKeys = query.Length == 0
            ? new List<string>()
            : query.Split('&', StringSplitOptions.RemoveEmptyEntries)
                .Select(part => part.Split('=')[0])
                .Where(key => key.Length > 0 && !key.Contains("{{", StringComparison.Ordinal))
                .Distinct(StringComparer.Ordinal)
                .ToList();

        return new ParsedUrl(origin, path, pathParams.Distinct(StringComparer.Ordinal).ToList(), queryKeys);
    }

    private static string Serialize(JsonNode node) => node.ToJsonString(new JsonSerializerOptions
    {
        WriteIndented = true,
        Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
    });
}
