using System.Text;
using System.Text.Json;

namespace Ricochet.Core;

/// <summary>A description that could not be read. The message is suitable for showing to the user.</summary>
public sealed class OpenApiException(string message) : Exception(message);

/// <summary>What an update did, listed before the user applies it.</summary>
public sealed record OpenApiSummary(
    bool IsNew,
    IReadOnlyList<string> Added,
    IReadOnlyList<string> Changed,
    IReadOnlyList<string> Removed,
    IReadOnlyList<string> Notes)
{
    public string ToReportText()
    {
        var lines = new List<string>
        {
            IsNew
                ? $"New collection with {Added.Count} requests."
                : $"{Added.Count} added, {Changed.Count} changed, {Removed.Count} no longer in the description.",
        };
        AddSection(lines, "Added", Added);
        AddSection(lines, "Changed", Changed);
        AddSection(lines, "No longer in the description (kept, so delete them by hand if you want)", Removed);
        AddSection(lines, "Not imported", Notes);
        return string.Join('\n', lines);
    }

    private static void AddSection(List<string> lines, string title, IReadOnlyList<string> items)
    {
        if (items.Count == 0)
            return;

        lines.Add("");
        lines.Add($"{title}:");
        lines.AddRange(items.Select(item => $"  {item}"));
    }
}

/// <summary>The Collection an OpenAPI description produces, with the list of what changed.</summary>
public sealed record OpenApiResult(Collection Collection, OpenApiSummary Summary);

/// <summary>
/// Turns an OpenAPI 3 description in JSON into a Collection, one Request per operation (ADR 0014).
/// Nothing here touches the network: see <see cref="OpenApiFetcher"/> for that.
/// </summary>
public static class OpenApiImporter
{
    private const string BaseUrlVariable = "baseUrl";
    private const int MaxDepth = 8;
    private static readonly string[] HttpMethods = ["get", "put", "post", "delete", "options", "head", "patch", "trace"];

    /// <param name="existing">The Collection an earlier update made from the same URL, or null to create one.</param>
    public static OpenApiResult Build(string json, string specUrl, Collection? existing)
    {
        JsonDocument document;
        try
        {
            document = JsonDocument.Parse(json);
        }
        catch (JsonException)
        {
            throw new OpenApiException("This is not JSON. Ricochet reads OpenAPI 3 in JSON, and YAML is not supported yet.");
        }

        using (document)
        {
            var root = document.RootElement;
            if (root.ValueKind != JsonValueKind.Object
                || !root.TryGetProperty("openapi", out var version)
                || version.ValueKind != JsonValueKind.String
                || !version.GetString()!.StartsWith("3.", StringComparison.Ordinal))
            {
                throw new OpenApiException("This is not an OpenAPI 3 description.");
            }

            return Build(new Reader(root), specUrl, existing);
        }
    }

    private static OpenApiResult Build(Reader reader, string specUrl, Collection? existing)
    {
        var root = reader.Root;
        var baseUrl = ServerUrl(reader);
        var schemes = reader.Get(reader.Get(root, "components"), "securitySchemes");
        var globalSecurity = root.TryGetProperty("security", out var global) ? global : default;

        reader.TagGroups = TagGroups(reader);
        var operations = ReadOperations(reader, schemes, globalSecurity);

        var collection = existing ?? new Collection { Name = Title(root, specUrl) };
        var variables = collection.Variables.ToList();
        AddVariable(variables, BaseUrlVariable, baseUrl, secret: false);
        foreach (var operation in operations)
            foreach (var variable in operation.AuthVariables)
                AddVariable(variables, variable.Key, variable.Value, variable.IsSecret);

        var byKey = operations.ToDictionary(o => o.Key);
        var matched = new HashSet<string>();
        var added = new List<string>();
        var changed = new List<string>();
        var removed = new List<string>();

        // Requests the description still has are updated in place, and the ones it no longer has are reported.
        IReadOnlyList<CollectionItem> Walk(IReadOnlyList<CollectionItem> items)
        {
            var result = new List<CollectionItem>();
            foreach (var item in items)
            {
                if (item.Request is null)
                {
                    result.Add(item with { Children = Walk(item.Children) });
                    continue;
                }

                var key = item.Request.SpecKey;
                if (key is null)
                {
                    result.Add(item);
                    continue;
                }

                if (!byKey.TryGetValue(key, out var operation))
                {
                    removed.Add(item.Name);
                    result.Add(item);
                    continue;
                }

                matched.Add(key);
                var request = Merge(item.Request, operation.Request);
                if (request != item.Request || item.Name != operation.Name)
                    changed.Add(operation.Name);
                result.Add(item with { Name = operation.Name, Request = request });
            }

            return result;
        }

        var items = Walk(collection.Items).ToList();

        // New operations go into the folders they belong to, created when needed.
        foreach (var operation in operations.Where(o => !matched.Contains(o.Key)))
        {
            Place(items, operation.Folders, new CollectionItem { Name = operation.Name, Request = operation.Request });
            added.Add(string.Join(" / ", operation.Folders.Append(operation.Name)));
        }

        var result = collection with
        {
            Items = items,
            Variables = variables,
            SpecUrl = specUrl,
        };

        var notes = reader.Notes.Distinct().ToList();
        var count = reader.OperationsWithUnaddedParameters;
        if (count == 1)
            notes.Add("1 operation has query or header parameters, which are not added yet. Add them by hand.");
        else if (count > 1)
            notes.Add($"{count} operations have query or header parameters, which are not added yet. Add them by hand.");

        return new OpenApiResult(result, new OpenApiSummary(existing is null, added, changed, removed, notes));
    }

    /// <summary>Puts an item inside the chain of folders it belongs to, creating folders as needed.</summary>
    private static void Place(List<CollectionItem> level, IReadOnlyList<string> folders, CollectionItem item)
    {
        if (folders.Count == 0)
        {
            level.Add(item);
            return;
        }

        var index = level.FindIndex(i => i.Request is null && i.Name == folders[0]);
        if (index < 0)
        {
            level.Add(new CollectionItem { Name = folders[0] });
            index = level.Count - 1;
        }

        var folder = level[index];
        var children = folder.Children.ToList();
        Place(children, folders.Skip(1).ToList(), item);
        level[index] = folder with { Children = children };
    }

    /// <summary>Reads x-tagGroups, used by some documentation tools to group tags, and maps each tag to its group.</summary>
    private static Dictionary<string, string> TagGroups(Reader reader)
    {
        var groups = new Dictionary<string, string>();
        var list = reader.Get(reader.Root, "x-tagGroups");
        if (list.ValueKind != JsonValueKind.Array)
            return groups;

        foreach (var group in list.EnumerateArray())
        {
            var name = Text(group, "name");
            var tags = reader.Get(group, "tags");
            if (name.Length == 0 || tags.ValueKind != JsonValueKind.Array)
                continue;

            foreach (var tag in tags.EnumerateArray())
            {
                if (tag.ValueKind == JsonValueKind.String)
                    groups.TryAdd(tag.GetString()!, name);
            }
        }

        return groups;
    }

    /// <summary>The first real word of a path, such as "account" for /v1/account. Null when the path has none.</summary>
    private static string? ResourceName(string path)
    {
        foreach (var part in path.Split('/', StringSplitOptions.RemoveEmptyEntries))
        {
            if (part.StartsWith('{'))
                return null;

            var isVersion = part.Length > 1 && part[0] == 'v' && part.Skip(1).All(char.IsDigit);
            if (part == "api" || isVersion)
                continue;

            return part;
        }

        return null;
    }

    private static List<Operation> ReadOperations(Reader reader, JsonElement schemes, JsonElement globalSecurity)
    {
        var operations = new List<Operation>();
        var seen = new HashSet<string>();
        var paths = reader.Get(reader.Root, "paths");
        if (paths.ValueKind != JsonValueKind.Object)
            return operations;

        foreach (var path in paths.EnumerateObject())
        {
            var pathItem = reader.Deref(path.Value);
            foreach (var method in HttpMethods)
            {
                var source = reader.Get(pathItem, method);
                if (source.ValueKind != JsonValueKind.Object)
                    continue;

                var operationId = Text(source, "operationId");
                var key = operationId.Length > 0 ? operationId : $"{method.ToUpperInvariant()} {path.Name}";
                if (!seen.Add(key))
                {
                    reader.Notes.Add($"The operation {key} appears more than once, so only the first was imported.");
                    continue;
                }

                operations.Add(ReadOperation(reader, key, method, path.Name, source, schemes, globalSecurity));
            }
        }

        return operations;
    }

    private static Operation ReadOperation(Reader reader, string key, string method, string path, JsonElement source, JsonElement schemes, JsonElement globalSecurity)
    {
        var summary = Text(source, "summary");
        var operationId = Text(source, "operationId");
        var name = summary.Length > 0 ? summary : operationId.Length > 0 ? operationId : $"{method.ToUpperInvariant()} {path}";
        var tags = reader.Get(source, "tags");
        var tag = tags.ValueKind == JsonValueKind.Array && tags.GetArrayLength() > 0 && tags[0].ValueKind == JsonValueKind.String
            ? tags[0].GetString()
            : null;

        // Folders, outermost first: the tag group if there is one, then the tag. Without tags, the path's resource.
        var folders = new List<string>();
        if (tag is not null && reader.TagGroups.TryGetValue(tag, out var group))
            folders.Add(group);
        var leaf = tag ?? ResourceName(path);
        if (leaf is not null)
            folders.Add(leaf);

        if (HasQueryOrHeaderParameters(reader, source))
            reader.OperationsWithUnaddedParameters++;

        var variables = new List<KeyValue>();
        var auth = ReadAuth(reader, source, schemes, globalSecurity, variables);
        var body = ReadBody(reader, source);

        var request = new ApiRequest
        {
            Name = name,
            Method = method.ToUpperInvariant(),
            // Path parameters become {{variables}}, so each one is listed as a placeholder until it has a value.
            Url = "{{" + BaseUrlVariable + "}}" + path.Replace("{", "{{", StringComparison.Ordinal).Replace("}", "}}", StringComparison.Ordinal),
            Auth = auth,
            Body = body ?? "",
            SpecKey = key,
        };

        return new Operation(key, name, folders, request, variables);
    }

    private static bool HasQueryOrHeaderParameters(Reader reader, JsonElement source)
    {
        var parameters = reader.Get(source, "parameters");
        if (parameters.ValueKind != JsonValueKind.Array)
            return false;

        foreach (var parameter in parameters.EnumerateArray())
        {
            var resolved = reader.Deref(parameter);
            var location = Text(resolved, "in");
            if (location is "query" or "header")
                return true;
        }

        return false;
    }

    /// <summary>The first security requirement decides the auth. Values are never copied: they become variables.</summary>
    private static RequestAuth? ReadAuth(Reader reader, JsonElement source, JsonElement schemes, JsonElement globalSecurity, List<KeyValue> variables)
    {
        var security = source.TryGetProperty("security", out var own) ? own : globalSecurity;
        if (security.ValueKind != JsonValueKind.Array || security.GetArrayLength() == 0)
            return null;

        var requirement = security[0];
        if (requirement.ValueKind != JsonValueKind.Object)
            return null;

        foreach (var first in requirement.EnumerateObject())
            return ReadScheme(reader, first.Name, schemes, variables);

        return null;
    }

    private static RequestAuth? ReadScheme(Reader reader, string schemeName, JsonElement schemes, List<KeyValue> variables)
    {
        var scheme = reader.Deref(reader.Get(schemes, schemeName));
        if (scheme.ValueKind != JsonValueKind.Object)
        {
            reader.Notes.Add($"The sign-in method {schemeName} is not defined in the description, so requests have no auth for it.");
            return null;
        }

        var variable = VariableName(schemeName);
        var type = Text(scheme, "type");
        var httpScheme = Text(scheme, "scheme");
        if (type == "http" && httpScheme.Equals("bearer", StringComparison.OrdinalIgnoreCase))
        {
            variables.Add(new KeyValue(variable, "", IsSecret: true));
            return new RequestAuth { Kind = AuthKinds.Bearer, Token = Placeholder(variable) };
        }

        if (type == "http" && httpScheme.Equals("basic", StringComparison.OrdinalIgnoreCase))
        {
            variables.Add(new KeyValue(variable + "Username", ""));
            variables.Add(new KeyValue(variable + "Password", "", IsSecret: true));
            return new RequestAuth
            {
                Kind = AuthKinds.Basic,
                Username = Placeholder(variable + "Username"),
                Password = Placeholder(variable + "Password"),
            };
        }

        if (type == "apiKey" && Text(scheme, "in") is "header" or "query")
        {
            variables.Add(new KeyValue(variable, "", IsSecret: true));
            return new RequestAuth
            {
                Kind = AuthKinds.ApiKey,
                KeyName = Text(scheme, "name"),
                KeyValue = Placeholder(variable),
                KeyInQuery = Text(scheme, "in") == "query",
            };
        }

        reader.Notes.Add($"The sign-in method {schemeName} ({type}) is not supported yet, so its requests have no auth. Set it by hand.");
        return null;
    }

    /// <summary>A JSON body is filled from the description's example, or built from its schema. Other types are reported.</summary>
    private static string? ReadBody(Reader reader, JsonElement source)
    {
        var content = reader.Get(reader.Get(source, "requestBody"), "content");
        if (content.ValueKind != JsonValueKind.Object)
            return null;

        foreach (var media in content.EnumerateObject())
        {
            if (!media.Name.StartsWith("application/json", StringComparison.OrdinalIgnoreCase))
                continue;

            using var stream = new MemoryStream();
            using (var writer = new Utf8JsonWriter(stream, new JsonWriterOptions { Indented = true }))
                WriteMediaValue(reader, writer, media.Value);
            return Encoding.UTF8.GetString(stream.ToArray());
        }

        reader.Notes.Add("Some requests have a body that is not JSON, so those bodies were not filled in.");
        return null;
    }

    private static void WriteMediaValue(Reader reader, Utf8JsonWriter writer, JsonElement media)
    {
        if (media.TryGetProperty("example", out var example))
        {
            example.WriteTo(writer);
            return;
        }

        var examples = reader.Get(media, "examples");
        if (examples.ValueKind == JsonValueKind.Object)
        {
            foreach (var firstExample in examples.EnumerateObject())
            {
                var value = reader.Get(firstExample.Value, "value");
                if (value.ValueKind != JsonValueKind.Undefined)
                {
                    value.WriteTo(writer);
                    return;
                }

                break;
            }
        }

        WriteSample(reader, writer, reader.Get(media, "schema"), 0);
    }

    /// <summary>Writes a value of the schema's shape, using examples, defaults or placeholders.</summary>
    private static void WriteSample(Reader reader, Utf8JsonWriter writer, JsonElement schema, int depth)
    {
        schema = reader.Deref(schema);
        if (schema.ValueKind != JsonValueKind.Object || depth > MaxDepth)
        {
            writer.WriteNullValue();
            return;
        }

        if (schema.TryGetProperty("example", out var example))
        {
            example.WriteTo(writer);
            return;
        }

        if (schema.TryGetProperty("default", out var fallback))
        {
            fallback.WriteTo(writer);
            return;
        }

        var choices = reader.Get(schema, "enum");
        if (choices.ValueKind == JsonValueKind.Array && choices.GetArrayLength() > 0)
        {
            choices[0].WriteTo(writer);
            return;
        }

        if (FirstOf(schema, "oneOf", out var option) || FirstOf(schema, "anyOf", out option))
        {
            WriteSample(reader, writer, option, depth + 1);
            return;
        }

        var type = Text(schema, "type");
        if (type.Length == 0 && (schema.TryGetProperty("properties", out _) || schema.TryGetProperty("allOf", out _)))
            type = "object";

        switch (type)
        {
            case "object":
                writer.WriteStartObject();
                WriteProperties(reader, writer, schema, depth);
                writer.WriteEndObject();
                break;
            case "array":
                writer.WriteStartArray();
                if (schema.TryGetProperty("items", out var items))
                    WriteSample(reader, writer, items, depth + 1);
                writer.WriteEndArray();
                break;
            case "integer":
            case "number":
                writer.WriteNumberValue(0);
                break;
            case "boolean":
                writer.WriteBooleanValue(false);
                break;
            case "string":
                writer.WriteStringValue("");
                break;
            default:
                writer.WriteNullValue();
                break;
        }
    }

    private static void WriteProperties(Reader reader, Utf8JsonWriter writer, JsonElement schema, int depth)
    {
        var sources = new List<JsonElement> { schema };
        if (schema.TryGetProperty("allOf", out var parts) && parts.ValueKind == JsonValueKind.Array)
            sources.AddRange(parts.EnumerateArray().Select(reader.Deref));

        foreach (var source in sources)
        {
            var properties = reader.Get(source, "properties");
            if (properties.ValueKind != JsonValueKind.Object)
                continue;

            foreach (var property in properties.EnumerateObject())
            {
                writer.WritePropertyName(property.Name);
                WriteSample(reader, writer, property.Value, depth + 1);
            }
        }
    }

    private static bool FirstOf(JsonElement schema, string keyword, out JsonElement first)
    {
        if (schema.TryGetProperty(keyword, out var options) && options.ValueKind == JsonValueKind.Array && options.GetArrayLength() > 0)
        {
            first = options[0];
            return true;
        }

        first = default;
        return false;
    }

    /// <summary>Keeps what the user has added or changed, and only refreshes what the description owns.</summary>
    private static ApiRequest Merge(ApiRequest existing, ApiRequest fromDescription)
    {
        var merged = existing with
        {
            Name = fromDescription.Name,
            Method = fromDescription.Method,
            Url = fromDescription.Url,
            Auth = fromDescription.Auth ?? existing.Auth,
        };

        // Body, headers, assertions and captures belong to the user once they exist, so they are left alone.
        return merged;
    }

    private static string ServerUrl(Reader reader)
    {
        var servers = reader.Get(reader.Root, "servers");
        if (servers.ValueKind != JsonValueKind.Array || servers.GetArrayLength() == 0)
        {
            reader.Notes.Add("The description lists no server, so set baseUrl by hand.");
            return "";
        }

        var url = Text(servers[0], "url").TrimEnd('/');
        if (!url.Contains("://", StringComparison.Ordinal))
            reader.Notes.Add("The server address is relative, so set baseUrl to the full address by hand.");
        else if (url.Contains('{', StringComparison.Ordinal))
            reader.Notes.Add("The server address has variables, so set baseUrl by hand.");

        return url;
    }

    private static string Title(JsonElement root, string specUrl)
    {
        var title = Text(Child(root, "info"), "title").Trim();
        if (title.Length > 0)
            return title;

        return Uri.TryCreate(specUrl, UriKind.Absolute, out var uri) ? uri.Host : "API";
    }

    private static void AddVariable(List<KeyValue> variables, string key, string value, bool secret) =>
        AddVariable(variables, new KeyValue(key, value, IsSecret: secret));

    /// <summary>Adds a Variable only when it is not there yet, so values the user typed are never overwritten.</summary>
    private static void AddVariable(List<KeyValue> variables, KeyValue variable)
    {
        if (!variables.Any(v => v.Key == variable.Key))
            variables.Add(variable);
    }

    private static string Placeholder(string name) => "{{" + name + "}}";

    private static string VariableName(string schemeName)
    {
        var chars = schemeName.Select(c => char.IsLetterOrDigit(c) ? c : '_').ToArray();
        return new string(chars);
    }

    private static string Text(JsonElement element, string name) =>
        element.ValueKind == JsonValueKind.Object && element.TryGetProperty(name, out var value) && value.ValueKind == JsonValueKind.String
            ? value.GetString() ?? ""
            : "";

    private static JsonElement Child(JsonElement element, string name) =>
        element.ValueKind == JsonValueKind.Object && element.TryGetProperty(name, out var value) ? value : default;

    private sealed record Operation(string Key, string Name, IReadOnlyList<string> Folders, ApiRequest Request, List<KeyValue> AuthVariables);

    /// <summary>Follows references inside the same document, and records the rest as not followed.</summary>
    private sealed class Reader(JsonElement root)
    {
        public JsonElement Root { get; } = root;
        public List<string> Notes { get; } = [];
        public int OperationsWithUnaddedParameters { get; set; }
        public Dictionary<string, string> TagGroups { get; set; } = new();

        public JsonElement Get(JsonElement element, string name) => Deref(Child(element, name));

        public JsonElement Deref(JsonElement element)
        {
            for (var hops = 0; hops < MaxDepth && element.ValueKind == JsonValueKind.Object; hops++)
            {
                if (!element.TryGetProperty("$ref", out var reference) || reference.ValueKind != JsonValueKind.String)
                    return element;

                var target = reference.GetString() ?? "";
                if (!target.StartsWith("#/", StringComparison.Ordinal))
                {
                    Notes.Add($"The reference {target} points outside this description, so it was not followed.");
                    return default;
                }

                var current = Root;
                foreach (var part in target[2..].Split('/'))
                {
                    var key = part.Replace("~1", "/", StringComparison.Ordinal).Replace("~0", "~", StringComparison.Ordinal);
                    if (!current.TryGetProperty(key, out current))
                        return default;
                }

                element = current;
            }

            return element;
        }
    }
}

/// <summary>Downloads an OpenAPI description. Only the user's request starts it (ADR 0014, ADR 0005).</summary>
public static class OpenApiFetcher
{
    public const int MaxBytes = 20 * 1024 * 1024;
    private static readonly TimeSpan Timeout = TimeSpan.FromSeconds(30);

    public static async Task<string> FetchAsync(string address, HttpClient client, CancellationToken cancellationToken = default)
    {
        if (!Uri.TryCreate(address.Trim(), UriKind.Absolute, out var uri) || uri.Scheme is not ("http" or "https"))
            throw new OpenApiException("Enter a web address that starts with http:// or https://.");

        using var timeout = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        timeout.CancelAfter(Timeout);

        try
        {
            using var response = await client.GetAsync(uri, HttpCompletionOption.ResponseHeadersRead, timeout.Token);
            if (!response.IsSuccessStatusCode)
                throw new OpenApiException($"The server answered {(int)response.StatusCode} {response.ReasonPhrase}.");

            if (response.Content.Headers.ContentLength > MaxBytes)
                throw new OpenApiException("The description is larger than 20 MB, so it was not read.");

            await using var stream = await response.Content.ReadAsStreamAsync(timeout.Token);
            using var buffer = new MemoryStream();
            var chunk = new byte[81920];
            int read;
            while ((read = await stream.ReadAsync(chunk, timeout.Token)) > 0)
            {
                if (buffer.Length + read > MaxBytes)
                    throw new OpenApiException("The description is larger than 20 MB, so it was not read.");

                buffer.Write(chunk, 0, read);
            }

            return Encoding.UTF8.GetString(buffer.GetBuffer(), 0, (int)buffer.Length);
        }
        catch (HttpRequestException ex)
        {
            throw new OpenApiException($"Could not fetch the description: {ex.Message}");
        }
        catch (OperationCanceledException) when (!cancellationToken.IsCancellationRequested)
        {
            throw new OpenApiException("The server took too long to answer.");
        }
    }
}
