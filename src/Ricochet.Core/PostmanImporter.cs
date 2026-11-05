using System.Text;
using System.Text.Json;
using System.Text.RegularExpressions;

namespace Ricochet.Core;

public sealed class PostmanImportException(string message) : Exception(message);

/// <summary>Something an import could not carry over, or that behaves differently here.</summary>
public sealed record ImportIssue(string Category, string Location, string Detail);

public static class ImportCategories
{
    public const string Scripts = "Scripts kept but not run";
    public const string DynamicVariables = "Dynamic variables that will not be resolved";
    public const string UnsupportedAuth = "Auth that was not imported";
    public const string DroppedBody = "Body content that was not imported";
    public const string DroppedData = "Other data that was not imported";
    public const string Secrets = "Variables marked as Secret, so kept out of your files";
}

public sealed record ImportResult
{
    public string Summary { get; init; } = "";
    public Collection? Collection { get; init; }
    public ApiEnvironment? Environment { get; init; }
    public IReadOnlyList<KeyValue>? Globals { get; init; }
    public IReadOnlyList<ImportIssue> Issues { get; init; } = [];

    public string ToReportText()
    {
        var text = new StringBuilder(Summary).AppendLine();
        foreach (var group in Issues.GroupBy(i => i.Category))
        {
            text.AppendLine().AppendLine($"{group.Key} ({group.Count()}):");
            foreach (var issue in group)
                text.AppendLine(issue.Detail.Length > 0 ? $"  {issue.Location}: {issue.Detail}" : $"  {issue.Location}");
        }

        return text.ToString();
    }
}

public static class WorkspaceImport
{
    /// <summary>Imports never overwrite an existing item of the same name, so a clash gets a numbered name.</summary>
    public static string UniqueName(string name, IEnumerable<string> existing)
    {
        var taken = existing.ToHashSet(StringComparer.OrdinalIgnoreCase);
        if (!taken.Contains(name))
            return name;

        for (var n = 2; ; n++)
        {
            var candidate = $"{name} ({n})";
            if (!taken.Contains(candidate))
                return candidate;
        }
    }

    public static (List<KeyValue> Merged, List<string> Overwritten) MergeGlobals(IEnumerable<KeyValue> existing, IEnumerable<KeyValue> incoming)
    {
        var merged = existing.ToList();
        var overwritten = new List<string>();
        foreach (var variable in incoming)
        {
            var index = merged.FindIndex(v => v.Key == variable.Key);
            if (index < 0)
            {
                merged.Add(variable);
                continue;
            }

            if (merged[index].Value != variable.Value)
                overwritten.Add(variable.Key);
            merged[index] = variable;
        }

        return (merged, overwritten);
    }
}

/// <summary>
/// Converts exported Postman files into our model. See docs/adr/0002-import-postman-exports.md.
/// Anything not carried over must be reported as an <see cref="ImportIssue"/>.
/// </summary>
public static partial class PostmanImporter
{
    private const int MaxJsonDepth = 512;
    private const string MultipartBoundary = "----RicochetFormBoundary";

    [GeneratedRegex(@"\{\{\s*(\$[^{}\s]+)\s*\}\}")]
    private static partial Regex DynamicVariable();

    [GeneratedRegex(@"(\{\{[^{}]+\}\})")]
    private static partial Regex PlaceholderSplit();

    /// <exception cref="PostmanImportException">The file is not an export we can read.</exception>
    public static ImportResult Import(string json, string fileName)
    {
        JsonDocument document;
        try
        {
            document = JsonDocument.Parse(json, new JsonDocumentOptions
            {
                MaxDepth = MaxJsonDepth,
                AllowTrailingCommas = true,
                CommentHandling = JsonCommentHandling.Skip,
            });
        }
        catch (JsonException ex)
        {
            throw new PostmanImportException($"Not valid JSON: {ex.Message}");
        }

        using (document)
        {
            var root = document.RootElement;
            if (root.ValueKind == JsonValueKind.Object)
            {
                if (Has(root, "info") && Has(root, "item"))
                    return new CollectionReader().Read(root, fileName);
                if (Array(root, "values") is { } values)
                    return ReadVariableFile(root, values, fileName);
                if (Has(root, "collections") || Has(root, "requests"))
                    throw new PostmanImportException("This uses Postman's older v1 or full-data-dump format. Export each collection as v2.1 and each environment separately.");
            }

            throw new PostmanImportException("This is not a Postman collection, environment or globals export.");
        }
    }

    private static ImportResult ReadVariableFile(JsonElement root, JsonElement values, string fileName)
    {
        var name = Text(root, "name") is { Length: > 0 } n ? n : Path.GetFileNameWithoutExtension(fileName);
        var isGlobals = Text(root, "_postman_variable_scope") == "globals";
        var location = isGlobals ? "Globals" : name;

        var issues = new List<ImportIssue>();
        var variables = new List<KeyValue>();
        foreach (var value in Elements(values))
        {
            var key = Text(value, "key");
            if (string.IsNullOrEmpty(key))
                continue;

            var enabled = !value.TryGetProperty("enabled", out var flag) || flag.ValueKind != JsonValueKind.False;
            variables.Add(new KeyValue(key, Text(value, "value") ?? "", enabled, IsSecretType(value)));
            NoteSecret(value, key, location, issues);
        }

        return isGlobals
            ? new ImportResult { Summary = $"Imported {Count(variables.Count, "global variable")}.", Globals = variables, Issues = issues }
            : new ImportResult
            {
                Summary = $"Imported environment \"{name}\" with {Count(variables.Count, "variable")}.",
                Environment = new ApiEnvironment { Name = name, Variables = variables },
                Issues = issues,
            };
    }

    private static bool IsSecretType(JsonElement variable) => Text(variable, "type") == "secret";

    private static void NoteSecret(JsonElement variable, string key, string location, List<ImportIssue> issues)
    {
        if (!IsSecretType(variable))
            return;

        var isEmpty = string.IsNullOrEmpty(Text(variable, "value"));
        issues.Add(new ImportIssue(ImportCategories.Secrets, location, isEmpty ? $"{key} (the export has no value, so set it again)" : key));
    }

    private sealed class CollectionReader
    {
        private readonly List<ImportIssue> _issues = [];
        private int _requests;
        private int _folders;
        private int _examples;
        private int _descriptions;

        public ImportResult Read(JsonElement root, string fileName)
        {
            var info = root.GetProperty("info");
            var schema = Text(info, "schema") ?? "";
            if (schema.Length > 0 && !schema.Contains("/v2.", StringComparison.Ordinal))
                throw new PostmanImportException($"Unsupported collection schema: {schema}. Export the collection as v2.1.");

            var name = Text(info, "name") is { Length: > 0 } n ? n : Path.GetFileNameWithoutExtension(fileName);
            if (Has(info, "description"))
                _descriptions++;

            var auth = ReadAuth(root, name, inherited: null);
            var items = ReadItems(root, name, auth);
            var variables = ReadCollectionVariables(root, name);
            var scripts = ReadScripts(root, name);

            if (_examples > 0)
                _issues.Add(new ImportIssue(ImportCategories.DroppedData, Count(_examples, "saved example response"), ""));
            if (_descriptions > 0)
                _issues.Add(new ImportIssue(ImportCategories.DroppedData, Count(_descriptions, "description"), ""));

            return new ImportResult
            {
                Summary = $"Imported collection \"{name}\": {Count(_requests, "request")}, {Count(_folders, "folder")}, {Count(variables.Count, "variable")}.",
                Collection = new Collection { Name = name, Items = items, Variables = variables, Scripts = scripts },
                Issues = _issues,
            };
        }

        private List<KeyValue> ReadCollectionVariables(JsonElement root, string location)
        {
            var variables = new List<KeyValue>();
            foreach (var variable in Elements(Array(root, "variable")))
            {
                var key = Text(variable, "key") is { Length: > 0 } k ? k : Text(variable, "id");
                if (string.IsNullOrEmpty(key))
                    continue;

                variables.Add(new KeyValue(key, Text(variable, "value") ?? "", !IsTrue(variable, "disabled"), IsSecretType(variable)));
                NoteSecret(variable, key, location, _issues);
            }

            return variables;
        }

        private List<CollectionItem> ReadItems(JsonElement parent, string parentPath, RequestAuth? inheritedAuth)
        {
            var items = new List<CollectionItem>();
            foreach (var item in Elements(Array(parent, "item")))
            {
                var name = Text(item, "name") ?? "Untitled";
                var path = $"{parentPath} / {name}";

                if (Has(item, "request"))
                {
                    items.Add(ReadRequestItem(item, name, path, inheritedAuth));
                }
                else if (Has(item, "item"))
                {
                    _folders++;
                    if (Has(item, "description"))
                        _descriptions++;

                    items.Add(new CollectionItem
                    {
                        Name = name,
                        Children = ReadItems(item, path, ReadAuth(item, path, inheritedAuth)),
                        Scripts = ReadScripts(item, path),
                    });
                }
                else
                {
                    _issues.Add(new ImportIssue(ImportCategories.DroppedData, path, "not a request or a folder"));
                }
            }

            return items;
        }

        private CollectionItem ReadRequestItem(JsonElement item, string name, string path, RequestAuth? inheritedAuth)
        {
            _requests++;
            _examples += Elements(Array(item, "response")).Count();

            var source = item.GetProperty("request");
            var request = source.ValueKind == JsonValueKind.String
                ? new ApiRequest { Name = name, Url = source.GetString() ?? "", Auth = inheritedAuth }
                : ReadRequest(source, name, path, inheritedAuth);

            NoteDynamicVariables(request, path);
            return new CollectionItem { Name = name, Request = request, Scripts = ReadScripts(item, path) };
        }

        private ApiRequest ReadRequest(JsonElement source, string name, string path, RequestAuth? inheritedAuth)
        {
            if (Has(source, "description"))
                _descriptions++;

            var headers = ReadHeaders(source);
            var body = ReadBody(source, path, headers);
            return new ApiRequest
            {
                Name = name,
                Method = (Text(source, "method") ?? "GET").ToUpperInvariant(),
                Url = ReadUrl(source, path),
                Headers = headers,
                Body = body,
                Auth = ReadAuth(source, path, inheritedAuth),
            };
        }

        private static List<KeyValue> ReadHeaders(JsonElement request)
        {
            var headers = new List<KeyValue>();
            if (!request.TryGetProperty("header", out var source))
                return headers;

            if (source.ValueKind == JsonValueKind.String)
            {
                foreach (var line in (source.GetString() ?? "").Split('\n', StringSplitOptions.RemoveEmptyEntries))
                {
                    var colon = line.IndexOf(':');
                    if (colon > 0)
                        headers.Add(new KeyValue(line[..colon].Trim(), line[(colon + 1)..].Trim()));
                }

                return headers;
            }

            foreach (var header in Elements(source))
            {
                if (Text(header, "key") is { Length: > 0 } key)
                    headers.Add(new KeyValue(key, Text(header, "value") ?? "", !IsTrue(header, "disabled")));
            }

            return headers;
        }

        private string ReadUrl(JsonElement request, string path)
        {
            if (!request.TryGetProperty("url", out var url))
                return "";

            if (url.ValueKind == JsonValueKind.String)
                return url.GetString() ?? "";
            if (url.ValueKind != JsonValueKind.Object)
                return "";

            var raw = Text(url, "raw");
            if (string.IsNullOrEmpty(raw))
                raw = BuildUrl(url);

            foreach (var query in Elements(Array(url, "query")))
            {
                if (IsTrue(query, "disabled"))
                    _issues.Add(new ImportIssue(ImportCategories.DroppedData, path, $"disabled query parameter \"{Text(query, "key")}\""));
            }

            foreach (var variable in Elements(Array(url, "variable")))
            {
                var key = Text(variable, "key");
                var value = Text(variable, "value");
                if (!string.IsNullOrEmpty(key) && !string.IsNullOrEmpty(value))
                    raw = Regex.Replace(raw, $@"(?<=/):{Regex.Escape(key)}(?=[/?#]|$)", value.Replace("$", "$$"));
            }

            return raw;
        }

        private static string BuildUrl(JsonElement url)
        {
            static string Join(JsonElement parent, string name, string separator)
            {
                if (!parent.TryGetProperty(name, out var part))
                    return "";

                return part.ValueKind == JsonValueKind.String
                    ? part.GetString() ?? ""
                    : string.Join(separator, Elements(part).Where(e => e.ValueKind == JsonValueKind.String).Select(e => e.GetString()));
            }

            var built = new StringBuilder();
            if (Text(url, "protocol") is { Length: > 0 } protocol)
                built.Append(protocol).Append("://");

            built.Append(Join(url, "host", "."));
            if (Text(url, "port") is { Length: > 0 } port)
                built.Append(':').Append(port);

            var path = Join(url, "path", "/");
            if (path.Length > 0)
                built.Append('/').Append(path.TrimStart('/'));

            var query = Elements(Array(url, "query"))
                .Where(q => !IsTrue(q, "disabled"))
                .Select(q => $"{Text(q, "key")}={Text(q, "value")}")
                .ToList();
            if (query.Count > 0)
                built.Append('?').AppendJoin('&', query);

            return built.ToString();
        }

        private string ReadBody(JsonElement request, string path, List<KeyValue> headers)
        {
            if (!request.TryGetProperty("body", out var body) || body.ValueKind != JsonValueKind.Object)
                return "";

            var mode = Text(body, "mode") ?? "";
            if (IsTrue(body, "disabled"))
            {
                _issues.Add(new ImportIssue(ImportCategories.DroppedBody, path, $"disabled {mode} body"));
                return "";
            }

            switch (mode)
            {
                case "":
                    return "";
                case "raw":
                    var language = body.TryGetProperty("options", out var options) && options.ValueKind == JsonValueKind.Object
                        && options.TryGetProperty("raw", out var rawOptions) ? Text(rawOptions, "language") : null;
                    var contentType = language switch
                    {
                        "json" => "application/json",
                        "xml" => "application/xml",
                        "html" => "text/html",
                        "javascript" => "application/javascript",
                        _ => null,
                    };
                    if (contentType is not null)
                        AddContentType(headers, contentType);
                    return Text(body, "raw") ?? "";
                case "urlencoded":
                    return ReadUrlEncoded(body, path, headers);
                case "formdata":
                    return ReadFormData(body, path, headers);
                case "graphql":
                    return ReadGraphQl(body, path, headers);
                case "file":
                    _issues.Add(new ImportIssue(ImportCategories.DroppedBody, path, "file upload"));
                    return "";
                default:
                    _issues.Add(new ImportIssue(ImportCategories.DroppedBody, path, $"unknown body type \"{mode}\""));
                    return "";
            }
        }

        private string ReadUrlEncoded(JsonElement body, string path, List<KeyValue> headers)
        {
            var pairs = new List<string>();
            foreach (var field in Elements(Array(body, "urlencoded")))
            {
                if (IsTrue(field, "disabled"))
                    _issues.Add(new ImportIssue(ImportCategories.DroppedBody, path, $"disabled form field \"{Text(field, "key")}\""));
                else
                    pairs.Add($"{EscapeKeepingPlaceholders(Text(field, "key") ?? "")}={EscapeKeepingPlaceholders(Text(field, "value") ?? "")}");
            }

            if (pairs.Count == 0)
                return "";

            AddContentType(headers, "application/x-www-form-urlencoded");
            return string.Join('&', pairs);
        }

        // Our body is text, so text fields are written as literal multipart content that stays visible and editable.
        private string ReadFormData(JsonElement body, string path, List<KeyValue> headers)
        {
            var multipart = new StringBuilder();
            foreach (var field in Elements(Array(body, "formdata")))
            {
                var key = Text(field, "key");
                if (Text(field, "type") == "file")
                {
                    _issues.Add(new ImportIssue(ImportCategories.DroppedBody, path, $"file form field \"{key}\""));
                }
                else if (IsTrue(field, "disabled"))
                {
                    _issues.Add(new ImportIssue(ImportCategories.DroppedBody, path, $"disabled form field \"{key}\""));
                }
                else
                {
                    multipart.Append($"--{MultipartBoundary}\r\nContent-Disposition: form-data; name=\"{key}\"\r\n");
                    if (Text(field, "contentType") is { Length: > 0 } partType)
                        multipart.Append($"Content-Type: {partType}\r\n");
                    multipart.Append($"\r\n{Text(field, "value")}\r\n");
                }
            }

            if (multipart.Length == 0)
                return "";

            AddContentType(headers, $"multipart/form-data; boundary={MultipartBoundary}");
            return multipart.Append($"--{MultipartBoundary}--\r\n").ToString();
        }

        private string ReadGraphQl(JsonElement body, string path, List<KeyValue> headers)
        {
            if (!body.TryGetProperty("graphql", out var graphql) || graphql.ValueKind != JsonValueKind.Object)
                return "";

            using var output = new MemoryStream();
            using (var writer = new Utf8JsonWriter(output, new JsonWriterOptions { Indented = true, Encoder = System.Text.Encodings.Web.JavaScriptEncoder.UnsafeRelaxedJsonEscaping }))
            {
                writer.WriteStartObject();
                writer.WriteString("query", Text(graphql, "query") ?? "");
                var variables = Text(graphql, "variables");
                if (!string.IsNullOrWhiteSpace(variables))
                {
                    try
                    {
                        using var parsed = JsonDocument.Parse(variables);
                        writer.WritePropertyName("variables");
                        parsed.WriteTo(writer);
                    }
                    catch (JsonException)
                    {
                        _issues.Add(new ImportIssue(ImportCategories.DroppedBody, path, "GraphQL variables that are not valid JSON"));
                    }
                }

                writer.WriteEndObject();
            }

            AddContentType(headers, "application/json");
            return Encoding.UTF8.GetString(output.GetBuffer(), 0, (int)output.Length);
        }

        private static void AddContentType(List<KeyValue> headers, string value)
        {
            if (!headers.Any(h => h.Enabled && h.Key.Equals("Content-Type", StringComparison.OrdinalIgnoreCase)))
                headers.Add(new KeyValue("Content-Type", value));
        }

        private static string EscapeKeepingPlaceholders(string text) =>
            string.Concat(PlaceholderSplit().Split(text).Select(part => part.StartsWith("{{", StringComparison.Ordinal) ? part : Uri.EscapeDataString(part)));

        // Inherited auth is resolved here, so each request holds the auth it will actually use.
        private RequestAuth? ReadAuth(JsonElement owner, string location, RequestAuth? inherited)
        {
            if (!owner.TryGetProperty("auth", out var auth) || auth.ValueKind != JsonValueKind.Object)
                return inherited;

            var type = Text(auth, "type") ?? "";
            var values = ReadAuthValues(auth, type);
            string Value(string key) => values.GetValueOrDefault(key, "");

            switch (type)
            {
                case "inherit":
                case "":
                    return inherited;
                case "noauth":
                    return null;
                case AuthKinds.Bearer:
                    return new RequestAuth { Kind = AuthKinds.Bearer, Token = Value("token") };
                case AuthKinds.Basic:
                    return new RequestAuth { Kind = AuthKinds.Basic, Username = Value("username"), Password = Value("password") };
                case AuthKinds.ApiKey:
                    return new RequestAuth { Kind = AuthKinds.ApiKey, KeyName = Value("key"), KeyValue = Value("value"), KeyInQuery = Value("in") == "query" };
                default:
                    _issues.Add(new ImportIssue(ImportCategories.UnsupportedAuth, location, type));
                    return null;
            }
        }

        // Schema v2.1 stores these as a list of key/value objects, v2.0 as one plain object.
        private static Dictionary<string, string> ReadAuthValues(JsonElement auth, string type)
        {
            var values = new Dictionary<string, string>();
            if (type.Length == 0 || !auth.TryGetProperty(type, out var source))
                return values;

            if (source.ValueKind == JsonValueKind.Array)
            {
                foreach (var entry in Elements(source))
                {
                    if (Text(entry, "key") is { Length: > 0 } key)
                        values[key] = Text(entry, "value") ?? "";
                }
            }
            else if (source.ValueKind == JsonValueKind.Object)
            {
                foreach (var property in source.EnumerateObject())
                    values[property.Name] = AsText(property.Value) ?? "";
            }

            return values;
        }

        private List<StoredScript> ReadScripts(JsonElement owner, string location)
        {
            var scripts = new List<StoredScript>();
            foreach (var entry in Elements(Array(owner, "event")))
            {
                if (!entry.TryGetProperty("script", out var script) || script.ValueKind != JsonValueKind.Object)
                    continue;

                var source = script.TryGetProperty("exec", out var exec) ? ReadExec(exec) : "";
                if (string.IsNullOrWhiteSpace(source))
                    continue; // Postman exports empty script stubs for many requests.

                var listen = Text(entry, "listen") ?? "script";
                scripts.Add(new StoredScript(listen, source));
                _issues.Add(new ImportIssue(ImportCategories.Scripts, location, listen));
            }

            return scripts;
        }

        private static string ReadExec(JsonElement exec) => exec.ValueKind == JsonValueKind.String
            ? exec.GetString() ?? ""
            : string.Join('\n', Elements(exec).Select(AsText));

        private void NoteDynamicVariables(ApiRequest request, string path)
        {
            var texts = new List<string> { request.Url, request.Body };
            texts.AddRange(request.Headers.Select(h => h.Value));
            if (request.Auth is { } auth)
                texts.AddRange([auth.Token, auth.Username, auth.Password, auth.KeyValue]);

            var names = texts.SelectMany(text => DynamicVariable().Matches(text)).Select(m => m.Groups[1].Value).Distinct().ToList();
            if (names.Count > 0)
                _issues.Add(new ImportIssue(ImportCategories.DynamicVariables, path, string.Join(", ", names)));
        }
    }

    private static string Count(int count, string noun) => count == 1 ? $"1 {noun}" : $"{count} {noun}s";

    private static bool Has(JsonElement element, string name) =>
        element.ValueKind == JsonValueKind.Object && element.TryGetProperty(name, out var value) && value.ValueKind != JsonValueKind.Null;

    private static JsonElement? Array(JsonElement element, string name) =>
        element.ValueKind == JsonValueKind.Object && element.TryGetProperty(name, out var value) && value.ValueKind == JsonValueKind.Array
            ? value
            : null;

    private static IEnumerable<JsonElement> Elements(JsonElement? array) =>
        array is { ValueKind: JsonValueKind.Array } items ? items.EnumerateArray() : [];

    private static bool IsTrue(JsonElement element, string name) =>
        element.ValueKind == JsonValueKind.Object && element.TryGetProperty(name, out var value) && value.ValueKind == JsonValueKind.True;

    private static string? Text(JsonElement element, string name) =>
        element.ValueKind == JsonValueKind.Object && element.TryGetProperty(name, out var value) ? AsText(value) : null;

    // Postman allows numbers and booleans where a string is usual, such as variable values.
    private static string? AsText(JsonElement value) => value.ValueKind switch
    {
        JsonValueKind.String => value.GetString(),
        JsonValueKind.Number or JsonValueKind.True or JsonValueKind.False => value.GetRawText(),
        _ => null,
    };
}
