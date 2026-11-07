using System.Text;
using System.Text.Json;
using System.Text.RegularExpressions;

namespace Ricochet.Core;

public sealed class InsomniaImportException(string message) : Exception(message);

/// <summary>
/// One Insomnia workspace. Its base environment becomes the Collection's variables, and each sub-environment becomes an Environment.
/// </summary>
public sealed record InsomniaWorkspaceImport(Collection Collection, IReadOnlyList<ApiEnvironment> Environments);

public sealed record InsomniaImportResult
{
    public string Summary { get; init; } = "";
    public IReadOnlyList<InsomniaWorkspaceImport> Workspaces { get; init; } = [];
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

/// <summary>
/// Converts Insomnia v4 exports into our model. See docs/adr/0033-import-insomnia-exports.md.
/// Anything not carried over must be reported as an <see cref="ImportIssue"/>.
/// </summary>
public static partial class InsomniaImporter
{
    private const int MaxJsonDepth = 512;
    private const string ExportFormatProperty = "__export_format";
    private const string MultipartBoundary = "----RicochetFormBoundary";

    // {{ name }} and {{ _.name }} (Insomnia's environment reference) both mean a Ricochet variable.
    [GeneratedRegex(@"\{\{\s*(?:_\.)?([A-Za-z_][\w-]*(?:\.[\w-]+)*)\s*\}\}")]
    private static partial Regex SimplePlaceholder();

    [GeneratedRegex(@"^[A-Za-z_][\w-]*(?:\.[\w-]+)*$")]
    private static partial Regex SimpleName();

    [GeneratedRegex(@"\{\{([^{}]*)\}\}")]
    private static partial Regex AnyPlaceholder();

    [GeneratedRegex(@"\{%.*?%\}", RegexOptions.Singleline)]
    private static partial Regex TagPlaceholder();

    [GeneratedRegex(@"\{%\s*uuid\s+'v4'\s*%\}")]
    private static partial Regex UuidTag();

    [GeneratedRegex(@"\{%\s*timestamp\s+'unix'\s*%\}")]
    private static partial Regex UnixTimestampTag();

    [GeneratedRegex(@"(\{\{[^{}]+\}\})")]
    private static partial Regex PlaceholderSplit();

    /// <exception cref="InsomniaImportException">The file is not an Insomnia export we can read.</exception>
    public static InsomniaImportResult Import(string json, string fileName)
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
            throw new InsomniaImportException($"Not valid JSON: {ex.Message}");
        }

        using (document)
        {
            var root = document.RootElement;
            if (root.ValueKind != JsonValueKind.Object || !root.TryGetProperty(ExportFormatProperty, out var format)
                || !root.TryGetProperty("resources", out var resources) || resources.ValueKind != JsonValueKind.Array)
            {
                throw new InsomniaImportException("This is not an Insomnia export.");
            }

            if (format.ValueKind != JsonValueKind.Number || !format.TryGetInt32(out var version) || version != 4)
            {
                throw new InsomniaImportException($"This is Insomnia export format {format.GetRawText()}, which is not supported. Export the workspace again from a current version of Insomnia, which writes format 4.");
            }

            var tree = new ResourceTree(resources);
            var workspaces = tree.OfType("workspace").ToList();
            if (workspaces.Count == 0)
                throw new InsomniaImportException("This Insomnia export has no workspace to import.");

            var issues = new List<ImportIssue>();
            var imports = new List<InsomniaWorkspaceImport>();
            var summaries = new List<string>();
            foreach (var workspace in workspaces)
            {
                var (import, summary) = new WorkspaceReader(tree, issues).Read(workspace, fileName);
                imports.Add(import);
                summaries.Add(summary);
            }

            return new InsomniaImportResult
            {
                Summary = string.Join('\n', summaries),
                Workspaces = imports,
                Issues = issues,
            };
        }
    }

    /// <summary>Reads one workspace. Each instance is used for one workspace only, so its counts describe that workspace.</summary>
    private sealed class WorkspaceReader(ResourceTree tree, List<ImportIssue> issues)
    {
        private readonly HashSet<string> _visited = [];
        private int _requests;
        private int _folders;
        private int _descriptions;

        public (InsomniaWorkspaceImport Import, string Summary) Read(JsonElement workspace, string fileName)
        {
            var id = Text(workspace, "_id") ?? "";
            var name = Text(workspace, "name") is { Length: > 0 } n ? n : Path.GetFileNameWithoutExtension(fileName);
            NoteDescription(workspace);

            var workspaceFound = new List<string>();
            var auth = ReadAuth(workspace, name, inherited: null, workspaceFound);
            ReportDynamic(workspaceFound, name);

            var items = ReadItems(id, name, auth, []);
            var environments = CollectEnvironments(id);

            var baseVariables = environments.Where(e => Text(e, "parentId") == id).SelectMany(e => ReadVariables(e, Text(e, "name") ?? "")).ToList();
            var subEnvironments = environments.Where(e => Text(e, "parentId") != id)
                .Select(e => new ApiEnvironment { Name = Text(e, "name") is { Length: > 0 } en ? en : "Untitled", Variables = ReadVariables(e, Text(e, "name") ?? "") })
                .ToList();

            if (_descriptions > 0)
                issues.Add(new ImportIssue(ImportCategories.DroppedData, Count(_descriptions, "description"), ""));

            var summary = $"Imported workspace \"{name}\": {Count(_requests, "request")}, {Count(_folders, "folder")}, {Count(baseVariables.Count, "variable")}, {Count(subEnvironments.Count, "environment")}.";
            var collection = new Collection { Name = name, Items = items, Variables = baseVariables };
            return (new InsomniaWorkspaceImport(collection, subEnvironments), summary);
        }

        // Environments sit under the workspace, and sub-environments under their base environment.
        private List<JsonElement> CollectEnvironments(string workspaceId)
        {
            var found = new List<JsonElement>();
            void Collect(string parentId)
            {
                foreach (var child in tree.Children(parentId))
                {
                    if (Text(child, "_type") != "environment")
                        continue;

                    var id = Text(child, "_id") ?? "";
                    if (id.Length > 0 && !_visited.Add(id))
                        continue;

                    found.Add(child);
                    if (id.Length > 0)
                        Collect(id);
                }
            }

            Collect(workspaceId);
            return found;
        }

        private List<CollectionItem> ReadItems(string parentId, string parentPath, RequestAuth? inheritedAuth, List<KeyValue> inheritedHeaders)
        {
            var items = new List<CollectionItem>();
            foreach (var child in tree.Children(parentId))
            {
                var type = Text(child, "_type") ?? "";
                if (type == "environment")
                    continue; // Read with the workspace's environments.

                var id = Text(child, "_id") ?? "";
                if (id.Length > 0 && !_visited.Add(id))
                    continue;

                var name = Text(child, "name") is { Length: > 0 } n ? n : "Untitled";
                var path = $"{parentPath} / {name}";
                switch (type)
                {
                    case "request":
                        items.Add(new CollectionItem { Name = name, Request = ReadRequest(child, name, path, inheritedAuth, inheritedHeaders) });
                        break;

                    case "request_group":
                    {
                        _folders++;
                        NoteDescription(child);
                        var folderAuth = ReadAuth(child, path, inheritedAuth, []);
                        var folderHeaders = ReadHeaders(child, path, out var folderFound);
                        ReportDynamic(folderFound, path);
                        var passed = Inherit(folderHeaders.Where(h => h.Enabled).ToList(), inheritedHeaders);
                        items.Add(new CollectionItem { Name = name, Children = ReadItems(id, path, folderAuth, passed) });
                        break;
                    }

                    case "cookie_jar":
                    {
                        var cookies = Array(child, "cookies")?.GetArrayLength() ?? 0;
                        if (cookies > 0)
                            issues.Add(new ImportIssue(ImportCategories.DroppedData, path, $"{Count(cookies, "cookie")} not imported"));
                        break;
                    }

                    default:
                        issues.Add(new ImportIssue(ImportCategories.DroppedData, path, $"{type} not imported"));
                        break;
                }
            }

            return items;
        }

        private ApiRequest ReadRequest(JsonElement source, string name, string path, RequestAuth? inheritedAuth, List<KeyValue> inheritedHeaders)
        {
            _requests++;
            NoteDescription(source);

            var found = new List<string>();
            var headers = Inherit(ReadHeaders(source, path, out var headerFound), inheritedHeaders);
            found.AddRange(headerFound);

            var url = Normalize(Text(source, "url") ?? "", found);
            foreach (var parameter in Elements(Array(source, "parameters")))
            {
                if (IsTrue(parameter, "disabled"))
                    issues.Add(new ImportIssue(ImportCategories.DroppedData, path, $"disabled query parameter \"{Text(parameter, "name")}\""));
            }

            var body = ReadBody(source, path, headers, found);
            var request = new ApiRequest
            {
                Name = name,
                Method = (Text(source, "method") ?? "GET").ToUpperInvariant(),
                Url = url,
                Headers = headers,
                Body = body,
                Auth = ReadAuth(source, path, inheritedAuth, found),
            };

            ReportDynamic(found, path);
            return request;
        }

        // Headers from a folder or workspace apply to requests inside it, unless the request sets a header with the same name.
        private static List<KeyValue> Inherit(List<KeyValue> own, List<KeyValue> inherited)
        {
            var names = own.Where(h => h.Enabled).Select(h => h.Key).ToHashSet(StringComparer.OrdinalIgnoreCase);
            return [.. own, .. inherited.Where(h => !names.Contains(h.Key))];
        }

        private static List<KeyValue> ReadHeaders(JsonElement owner, string location, out List<string> found)
        {
            found = [];
            var headers = new List<KeyValue>();
            foreach (var header in Elements(Array(owner, "headers")))
            {
                var name = Text(header, "name") ?? "";
                if (name.Length == 0)
                    continue;

                headers.Add(new KeyValue(Normalize(name, found), Normalize(Text(header, "value") ?? "", found), !IsTrue(header, "disabled")));
            }

            return headers;
        }

        private string ReadBody(JsonElement request, string path, List<KeyValue> headers, List<string> found)
        {
            if (!request.TryGetProperty("body", out var body) || body.ValueKind != JsonValueKind.Object)
                return "";

            if (Text(body, "fileName") is { Length: > 0 })
            {
                issues.Add(new ImportIssue(ImportCategories.DroppedBody, path, "file upload"));
                return "";
            }

            var mime = Text(body, "mimeType") ?? "";
            switch (mime)
            {
                case "application/x-www-form-urlencoded":
                    return ReadUrlEncoded(body, path, headers, found);
                case "multipart/form-data":
                    return ReadMultipart(body, path, headers, found);
            }

            var text = Normalize(Text(body, "text") ?? "", found);
            if (text.Length == 0)
                return "";

            if (mime.Length > 0)
                AddContentType(headers, mime);
            return text;
        }

        private string ReadUrlEncoded(JsonElement body, string path, List<KeyValue> headers, List<string> found)
        {
            var pairs = new List<string>();
            foreach (var field in Elements(Array(body, "params")))
            {
                var key = Normalize(Text(field, "name") ?? "", found);
                if (IsTrue(field, "disabled"))
                {
                    issues.Add(new ImportIssue(ImportCategories.DroppedBody, path, $"disabled form field \"{key}\""));
                    continue;
                }

                var value = Normalize(Text(field, "value") ?? "", found);
                pairs.Add($"{EscapeKeepingPlaceholders(key)}={EscapeKeepingPlaceholders(value)}");
            }

            if (pairs.Count == 0)
                return "";

            AddContentType(headers, "application/x-www-form-urlencoded");
            return string.Join('&', pairs);
        }

        // Our body is text, so text fields are written as literal multipart content that stays visible and editable.
        private string ReadMultipart(JsonElement body, string path, List<KeyValue> headers, List<string> found)
        {
            var multipart = new StringBuilder();
            foreach (var field in Elements(Array(body, "params")))
            {
                var key = Text(field, "name") ?? "";
                if (Text(field, "type") == "file" || Text(field, "fileName") is { Length: > 0 })
                {
                    issues.Add(new ImportIssue(ImportCategories.DroppedBody, path, $"file form field \"{key}\""));
                }
                else if (IsTrue(field, "disabled"))
                {
                    issues.Add(new ImportIssue(ImportCategories.DroppedBody, path, $"disabled form field \"{key}\""));
                }
                else
                {
                    var value = Normalize(Text(field, "value") ?? "", found);
                    multipart.Append($"--{MultipartBoundary}\r\nContent-Disposition: form-data; name=\"{key}\"\r\n\r\n{value}\r\n");
                }
            }

            if (multipart.Length == 0)
                return "";

            AddContentType(headers, $"multipart/form-data; boundary={MultipartBoundary}");
            return multipart.Append($"--{MultipartBoundary}--\r\n").ToString();
        }

        // Inherited auth is resolved here, so each request holds the auth it will actually use.
        private RequestAuth? ReadAuth(JsonElement owner, string location, RequestAuth? inherited, List<string> found)
        {
            if (!owner.TryGetProperty("authentication", out var auth) || auth.ValueKind != JsonValueKind.Object)
                return inherited;

            var type = Text(auth, "type") ?? "";
            if (type is "" or "inherit")
                return inherited;
            if (type == "none")
                return null;
            if (IsTrue(auth, "disabled"))
            {
                issues.Add(new ImportIssue(ImportCategories.DroppedData, location, $"disabled {type} auth"));
                return null;
            }

            string Value(string key) => Normalize(Text(auth, key) ?? "", found);
            switch (type)
            {
                case AuthKinds.Bearer:
                {
                    var prefix = Text(auth, "prefix") ?? "";
                    if (prefix.Length > 0 && !prefix.Equals("Bearer", StringComparison.OrdinalIgnoreCase))
                        issues.Add(new ImportIssue(ImportCategories.UnsupportedAuth, location, $"bearer prefix \"{prefix}\" (the token is sent with Bearer)"));
                    return new RequestAuth { Kind = AuthKinds.Bearer, Token = Value("token") };
                }

                case AuthKinds.Basic:
                    return new RequestAuth { Kind = AuthKinds.Basic, Username = Value("username"), Password = Value("password") };

                case "apikey":
                    return new RequestAuth
                    {
                        Kind = AuthKinds.ApiKey,
                        KeyName = Value("key"),
                        KeyValue = Value("value"),
                        KeyInQuery = Text(auth, "addTo") == "queryParams",
                    };

                default:
                    issues.Add(new ImportIssue(ImportCategories.UnsupportedAuth, location, type));
                    return null;
            }
        }

        private List<KeyValue> ReadVariables(JsonElement environment, string location)
        {
            var found = new List<string>();
            var variables = new List<KeyValue>();
            if (environment.TryGetProperty("data", out var data) && data.ValueKind == JsonValueKind.Object)
            {
                foreach (var property in data.EnumerateObject())
                    variables.Add(new KeyValue(property.Name, Normalize(VariableText(property.Value), found)));
            }

            ReportDynamic(found, location);
            return variables;
        }

        private void NoteDescription(JsonElement element)
        {
            if (Text(element, "description") is { Length: > 0 })
                _descriptions++;
        }

        private void ReportDynamic(List<string> found, string location)
        {
            if (found.Count > 0)
                issues.Add(new ImportIssue(ImportCategories.DynamicVariables, location, string.Join(", ", found.Distinct())));
        }
    }

    /// <summary>
    /// Rewrites Insomnia placeholders to ours. Names that do not map are left as written and returned in <paramref name="found"/>
    /// so the import report can list them.
    /// </summary>
    private static string Normalize(string text, List<string> found)
    {
        if (text.Length == 0)
            return text;

        text = UuidTag().Replace(text, "{{$guid}}");
        text = UnixTimestampTag().Replace(text, "{{$timestamp}}");
        text = SimplePlaceholder().Replace(text, m => "{{" + m.Groups[1].Value + "}}");

        foreach (Match tag in TagPlaceholder().Matches(text))
            found.Add(tag.Value);

        foreach (Match tag in AnyPlaceholder().Matches(text))
        {
            var inner = tag.Groups[1].Value.Trim();
            if (!SimpleName().IsMatch(inner) && !BuiltInFunctions.IsPostmanAlias(inner))
                found.Add(inner);
        }

        return text;
    }

    private static string EscapeKeepingPlaceholders(string text) =>
        string.Concat(PlaceholderSplit().Split(text).Select(part => part.StartsWith("{{", StringComparison.Ordinal) ? part : Uri.EscapeDataString(part)));

    private static void AddContentType(List<KeyValue> headers, string value)
    {
        if (!headers.Any(h => h.Enabled && h.Key.Equals("Content-Type", StringComparison.OrdinalIgnoreCase)))
            headers.Add(new KeyValue("Content-Type", value));
    }

    /// <summary>Resources in an export, grouped by parent and ordered by Insomnia's sort key.</summary>
    private sealed class ResourceTree
    {
        private readonly Dictionary<string, List<JsonElement>> _children = [];
        private readonly List<JsonElement> _all = [];

        public ResourceTree(JsonElement resources)
        {
            foreach (var resource in resources.EnumerateArray())
            {
                if (resource.ValueKind != JsonValueKind.Object)
                    continue;

                _all.Add(resource);
                var parent = Text(resource, "parentId") ?? "";
                if (!_children.TryGetValue(parent, out var list))
                    _children[parent] = list = [];
                list.Add(resource);
            }

            foreach (var key in _children.Keys.ToList())
                _children[key] = _children[key].OrderBy(SortKey).ToList();
        }

        public IEnumerable<JsonElement> OfType(string type) => _all.Where(r => Text(r, "_type") == type).OrderBy(SortKey);

        public IEnumerable<JsonElement> Children(string parentId) =>
            _children.TryGetValue(parentId, out var list) ? list : [];

        private static double SortKey(JsonElement resource) =>
            resource.TryGetProperty("metaSortKey", out var key) && key.ValueKind == JsonValueKind.Number ? key.GetDouble() : 0;
    }

    private static string VariableText(JsonElement value) => value.ValueKind switch
    {
        JsonValueKind.String => value.GetString() ?? "",
        JsonValueKind.Null => "",
        _ => value.GetRawText(), // Numbers, booleans, and the objects or arrays Insomnia allows as values.
    };

    private static string Count(int count, string noun) => count == 1 ? $"1 {noun}" : $"{count} {noun}s";

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

    // Insomnia allows numbers and booleans where a string is usual, such as environment values.
    private static string? AsText(JsonElement value) => value.ValueKind switch
    {
        JsonValueKind.String => value.GetString(),
        JsonValueKind.Number or JsonValueKind.True or JsonValueKind.False => value.GetRawText(),
        _ => null,
    };
}
