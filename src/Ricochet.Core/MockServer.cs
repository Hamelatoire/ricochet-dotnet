using System.Net;
using System.Text;
using System.Text.Json;

namespace Ricochet.Core;

/// <summary>A problem that stops the mock server from starting or reading a description. The message is suitable for showing to the user.</summary>
public sealed class MockServerException(string message) : Exception(message);

/// <summary>
/// One response a mock can give: a method and URL (or path) and the reply to send.
/// Saved examples (ADR 0023) become these, and so do operations from an OpenAPI description (ADR 0027).
/// </summary>
public sealed record MockExample(string Method, string Url, int StatusCode, string? MediaType, string Body, IReadOnlyList<KeyValue>? Headers = null);

public sealed record MockReply(int StatusCode, IReadOnlyList<KeyValue> Headers, byte[] Body, string? MediaType);

/// <summary>A request that was answered, as shown in the log. It holds the path only, never the query string or headers, which can carry secrets.</summary>
public sealed record MockLogEntry(DateTimeOffset At, string Method, string Path, int StatusCode);

/// <summary>
/// A method and path that a mock answers. Segments in braces (<c>{id}</c>), in double braces (<c>{{id}}</c>)
/// or after a colon (<c>:id</c>) match any single segment.
/// </summary>
public sealed class MockRoute
{
    public MockRoute(string method, string path, MockReply reply)
    {
        Method = method.ToUpperInvariant();
        Path = path;
        Reply = reply;
        Segments = MockPaths.Segments(path);
        LiteralCount = Segments.Count(s => !MockPaths.IsParameter(s));
    }

    public string Method { get; }
    public string Path { get; }
    public MockReply Reply { get; }
    public IReadOnlyList<string> Segments { get; }

    /// <summary>How many segments are literal text. A route with more literal text is more specific, and wins.</summary>
    public int LiteralCount { get; }
}

/// <summary>Decides which route answers a request. Pure: no sockets, so it can be tested directly.</summary>
public sealed class MockRouteTable(IEnumerable<MockRoute> routes)
{
    private readonly IReadOnlyList<MockRoute> _routes = routes.ToList();

    public int Count => _routes.Count;

    /// <summary>The route that answers a request, or null. The query string and fragment are ignored.</summary>
    public MockRoute? Find(string method, string target) => Resolve(method, target).Route;

    /// <summary>The reply for a request. Unknown paths get a 404 and known paths with another method get a 405, both as JSON.</summary>
    public MockReply Respond(string method, string target)
    {
        var (route, allowed) = Resolve(method, target);
        if (route is not null)
            return route.Reply;

        var path = MockPaths.PathOf(target);
        if (allowed.Count > 0)
        {
            var allow = string.Join(", ", allowed);
            return ErrorReply(405, $"{method.ToUpperInvariant()} is not allowed on {path}. Use {allow}.",
                [new KeyValue("Allow", allow)]);
        }

        return ErrorReply(404, $"No mock matches {method.ToUpperInvariant()} {path}.", []);
    }

    private (MockRoute? Route, SortedSet<string> Allowed) Resolve(string method, string target)
    {
        var segments = MockPaths.Segments(MockPaths.PathOf(target));
        var upperMethod = method.ToUpperInvariant();
        MockRoute? best = null;
        var allowed = new SortedSet<string>(StringComparer.Ordinal);

        foreach (var route in _routes)
        {
            if (!MockPaths.Matches(route.Segments, segments))
                continue;

            if (route.Method == upperMethod)
            {
                // Strictly more literal text wins, so the first registered route wins a tie.
                if (best is null || route.LiteralCount > best.LiteralCount)
                    best = route;
            }
            else
            {
                allowed.Add(route.Method);
            }
        }

        return (best, allowed);
    }

    private static MockReply ErrorReply(int status, string message, IReadOnlyList<KeyValue> headers)
    {
        var buffer = new MemoryStream();
        using (var writer = new Utf8JsonWriter(buffer))
        {
            writer.WriteStartObject();
            writer.WriteString("error", message);
            writer.WriteString("source", "Ricochet mock server");
            writer.WriteEndObject();
        }

        return new MockReply(status, headers, buffer.ToArray(), "application/json; charset=utf-8");
    }
}

/// <summary>
/// Listens on 127.0.0.1 only, so nothing else on the network can reach it (ADR 0027). It opens no outgoing
/// connections, and it runs only between <see cref="Start"/> and <see cref="Stop"/>.
/// </summary>
public sealed class MockServer
{
    public const int MinPort = 1024;
    public const int MaxPort = 65535;

    private readonly object _gate = new();
    private HttpListener? _listener;
    private volatile MockRouteTable _routes = new([]);

    /// <summary>Raised on a background thread after each request is answered.</summary>
    public event Action<MockLogEntry>? RequestHandled;

    public bool IsRunning
    {
        get
        {
            lock (_gate)
                return _listener is not null;
        }
    }

    /// <summary>The port in use while running, or 0.</summary>
    public int Port { get; private set; }

    /// <exception cref="MockServerException">The port is out of range, already in use, or the server is already running.</exception>
    public void Start(int port, MockRouteTable routes)
    {
        if (port is < MinPort or > MaxPort)
            throw new MockServerException($"Choose a port from {MinPort} to {MaxPort}.");

        lock (_gate)
        {
            if (_listener is not null)
                throw new MockServerException("The mock server is already running.");

            var listener = new HttpListener();
            listener.Prefixes.Add($"http://127.0.0.1:{port}/");
            try
            {
                listener.Start();
            }
            catch (HttpListenerException)
            {
                listener.Close();
                throw new MockServerException($"Port {port} could not be used. Another program may already be using it.");
            }

            _routes = routes;
            _listener = listener;
            Port = port;
            _ = Task.Run(() => ServeAsync(listener));
        }
    }

    /// <summary>Swaps the routes for the requests that arrive from now on.</summary>
    public void UpdateRoutes(MockRouteTable routes) => _routes = routes;

    public void Stop()
    {
        lock (_gate)
        {
            if (_listener is null)
                return;

            _listener.Close();
            _listener = null;
            Port = 0;
        }
    }

    private async Task ServeAsync(HttpListener listener)
    {
        while (true)
        {
            HttpListenerContext context;
            try
            {
                context = await listener.GetContextAsync();
            }
            catch (Exception)
            {
                // Stop() closes the listener, which ends the loop. Any other failure ends it too, since the port is no longer served.
                return;
            }

            try
            {
                await AnswerAsync(context);
            }
            catch (Exception)
            {
                // A client that drops mid-reply must not stop the server.
                try { context.Response.Abort(); } catch (Exception) { }
            }
        }
    }

    private async Task AnswerAsync(HttpListenerContext context)
    {
        var method = context.Request.HttpMethod;
        var target = context.Request.RawUrl ?? "/";
        var reply = _routes.Respond(method, target);

        var response = context.Response;
        response.StatusCode = reply.StatusCode;
        foreach (var header in reply.Headers)
        {
            try
            {
                response.Headers[header.Key] = header.Value;
            }
            catch (ArgumentException)
            {
                // Headers the listener owns, such as Content-Length, are skipped.
            }
        }

        if (reply.MediaType is not null)
            response.ContentType = reply.MediaType;
        response.ContentLength64 = reply.Body.Length;
        await response.OutputStream.WriteAsync(reply.Body);
        response.Close();

        RequestHandled?.Invoke(new MockLogEntry(DateTimeOffset.Now, method.ToUpperInvariant(), MockPaths.PathOf(target), reply.StatusCode));
    }
}

/// <summary>Path handling shared by the router, the server and the builders.</summary>
public static class MockPaths
{
    private static readonly System.Text.RegularExpressions.Regex ParameterPattern =
        new(@"^(\{\{[^}]+\}\}|\{[^{}]+\}|:[A-Za-z_][\w-]*)$", System.Text.RegularExpressions.RegexOptions.CultureInvariant);

    /// <summary>
    /// The path of a request target or URL. Scheme and host are dropped, and so are the query and fragment.
    /// A leading variable such as {{baseUrl}} is dropped too, since the mock cannot know what it stands for.
    /// </summary>
    public static string PathOf(string target)
    {
        var text = target;
        var cut = text.IndexOfAny(['?', '#']);
        if (cut >= 0)
            text = text[..cut];

        var schemeEnd = text.IndexOf("://", StringComparison.Ordinal);
        if (schemeEnd >= 0)
        {
            var afterAuthority = text[(schemeEnd + 3)..];
            var slash = afterAuthority.IndexOf('/');
            text = slash < 0 ? "/" : afterAuthority[slash..];
        }
        else if (text.StartsWith("{{", StringComparison.Ordinal))
        {
            var close = text.IndexOf("}}", StringComparison.Ordinal);
            text = close < 0 ? "/" : text[(close + 2)..];
        }

        return text.StartsWith('/') ? text : "/" + text;
    }

    public static IReadOnlyList<string> Segments(string path) =>
        path.Split('/', StringSplitOptions.RemoveEmptyEntries);

    public static bool IsParameter(string segment) => ParameterPattern.IsMatch(segment);

    public static bool Matches(IReadOnlyList<string> template, IReadOnlyList<string> segments)
    {
        if (template.Count != segments.Count)
            return false;

        for (var i = 0; i < template.Count; i++)
        {
            if (!IsParameter(template[i]) && !string.Equals(template[i], segments[i], StringComparison.Ordinal))
                return false;
        }

        return true;
    }
}

/// <summary>Turns saved examples or an OpenAPI description into routes. Nothing here touches the network or the disk.</summary>
public static class MockRouteBuilder
{
    /// <summary>Examples with the same method and path as an earlier one are skipped, so the first wins.</summary>
    public static IReadOnlyList<MockRoute> FromExamples(IEnumerable<MockExample> examples)
    {
        var routes = new List<MockRoute>();
        var seen = new HashSet<string>(StringComparer.Ordinal);
        foreach (var example in examples)
        {
            var path = MockPaths.PathOf(example.Url);
            var method = example.Method.ToUpperInvariant();
            if (!seen.Add(method + " " + path))
                continue;

            var headers = example.Headers ?? Array.Empty<KeyValue>();
            routes.Add(new MockRoute(method, path, new MockReply(example.StatusCode, headers, Encoding.UTF8.GetBytes(example.Body), example.MediaType)));
        }

        return routes;
    }

    /// <summary>
    /// One route per operation in an OpenAPI 3 JSON description. Each answers with its first 2xx response: the
    /// example in the description if there is one, otherwise a sample built from its schema. Paths are served
    /// under the path of the first server, so <c>servers: [ { url: "https://api/v1" } ]</c> serves <c>/v1/pets</c>.
    /// </summary>
    /// <exception cref="MockServerException">The text is not an OpenAPI 3 description in JSON.</exception>
    public static IReadOnlyList<MockRoute> FromOpenApi(string json)
    {
        JsonDocument document;
        try
        {
            document = JsonDocument.Parse(json);
        }
        catch (JsonException)
        {
            throw new MockServerException("This is not JSON. Ricochet reads OpenAPI 3 in JSON, and YAML is not supported yet.");
        }

        using (document)
        {
            var root = document.RootElement;
            if (root.ValueKind != JsonValueKind.Object
                || !root.TryGetProperty("openapi", out var version)
                || version.ValueKind != JsonValueKind.String
                || !version.GetString()!.StartsWith("3.", StringComparison.Ordinal))
            {
                throw new MockServerException("This is not an OpenAPI 3 description.");
            }

            if (!root.TryGetProperty("paths", out var paths) || paths.ValueKind != JsonValueKind.Object)
                throw new MockServerException("The description has no paths, so there is nothing to mock.");

            var basePath = BasePath(root);
            var routes = new List<MockRoute>();
            var seen = new HashSet<string>(StringComparer.Ordinal);
            foreach (var pathItem in paths.EnumerateObject())
            {
                if (pathItem.Value.ValueKind != JsonValueKind.Object)
                    continue;

                foreach (var method in OpenApiMethods)
                {
                    if (!pathItem.Value.TryGetProperty(method, out var operation) || operation.ValueKind != JsonValueKind.Object)
                        continue;

                    var fullPath = basePath + pathItem.Name;
                    var upper = method.ToUpperInvariant();
                    if (!seen.Add(upper + " " + fullPath))
                        continue;

                    routes.Add(new MockRoute(upper, fullPath, ReplyFor(operation, root)));
                }
            }

            return routes;
        }
    }

    private static readonly string[] OpenApiMethods = ["get", "put", "post", "delete", "options", "head", "patch", "trace"];
    private const int MaxSampleDepth = 8;

    private static string BasePath(JsonElement root)
    {
        if (!root.TryGetProperty("servers", out var servers) || servers.ValueKind != JsonValueKind.Array)
            return "";

        var first = servers.EnumerateArray().FirstOrDefault();
        if (first.ValueKind != JsonValueKind.Object || !first.TryGetProperty("url", out var url) || url.ValueKind != JsonValueKind.String)
            return "";

        var path = MockPaths.PathOf(url.GetString()!).TrimEnd('/');
        return path == "/" ? "" : path;
    }

    private static MockReply ReplyFor(JsonElement operation, JsonElement root)
    {
        if (!operation.TryGetProperty("responses", out var responses) || responses.ValueKind != JsonValueKind.Object)
            return new MockReply(200, [], [], null);

        // Lowest 2xx first, then "2XX", then "default", which is answered as 200.
        int? status = null;
        JsonElement chosen = default;
        foreach (var response in responses.EnumerateObject())
        {
            var key = response.Name;
            if (key.Length == 3 && key[0] == '2' && int.TryParse(key, out var code) && (status is null || code < status))
            {
                status = code;
                chosen = response.Value;
            }
        }

        if (status is null)
        {
            if (responses.TryGetProperty("2XX", out var range))
            {
                status = 200;
                chosen = range;
            }
            else if (responses.TryGetProperty("default", out var fallback))
            {
                status = 200;
                chosen = fallback;
            }
            else
            {
                return new MockReply(200, [], [], null);
            }
        }

        var (mediaType, body) = BodyFor(Deref(chosen, root, 0) ?? default, root);
        return new MockReply(status.Value, [], body, mediaType);
    }

    private static (string? MediaType, byte[] Body) BodyFor(JsonElement response, JsonElement root)
    {
        if (response.ValueKind != JsonValueKind.Object
            || !response.TryGetProperty("content", out var content)
            || content.ValueKind != JsonValueKind.Object)
        {
            return (null, []);
        }

        var types = content.EnumerateObject().Select(p => p.Name).ToList();
        var pick = types.FirstOrDefault(n => n.Equals("application/json", StringComparison.OrdinalIgnoreCase))
                   ?? types.FirstOrDefault(n => n.EndsWith("/json", StringComparison.OrdinalIgnoreCase) || n.EndsWith("+json", StringComparison.OrdinalIgnoreCase));
        if (pick is null)
        {
            // Not JSON: only a text example can be sent.
            if (types.Count == 0 || content.GetProperty(types[0]) is not { ValueKind: JsonValueKind.Object } other)
                return (null, []);
            var text = other.TryGetProperty("example", out var raw) && raw.ValueKind == JsonValueKind.String ? raw.GetString()! : "";
            return (types[0], Encoding.UTF8.GetBytes(text));
        }

        var media = content.GetProperty(pick);
        var buffer = new MemoryStream();
        using (var writer = new Utf8JsonWriter(buffer))
        {
            if (media.ValueKind == JsonValueKind.Object && media.TryGetProperty("example", out var example))
            {
                example.WriteTo(writer);
            }
            else if (media.ValueKind == JsonValueKind.Object
                     && media.TryGetProperty("examples", out var examples)
                     && examples.ValueKind == JsonValueKind.Object
                     && FirstProperty(examples) is { } firstExample
                     && firstExample.Value.ValueKind == JsonValueKind.Object
                     && firstExample.Value.TryGetProperty("value", out var value))
            {
                value.WriteTo(writer);
            }
            else if (media.ValueKind == JsonValueKind.Object && media.TryGetProperty("schema", out var schema))
            {
                WriteSample(writer, schema, root, 0);
            }
            else
            {
                writer.WriteNullValue();
            }
        }

        return (pick, buffer.ToArray());
    }

    private static JsonProperty? FirstProperty(JsonElement obj)
    {
        foreach (var property in obj.EnumerateObject())
            return property;
        return null;
    }

    private static void WriteSample(Utf8JsonWriter writer, JsonElement schema, JsonElement root, int depth)
    {
        var node = Deref(schema, root, 0);
        if (node is not { ValueKind: JsonValueKind.Object } || depth > MaxSampleDepth)
        {
            writer.WriteNullValue();
            return;
        }

        var s = node.Value;

        if (s.TryGetProperty("example", out var example))
        {
            example.WriteTo(writer);
            return;
        }

        if (s.TryGetProperty("default", out var defaultValue))
        {
            defaultValue.WriteTo(writer);
            return;
        }

        if (s.TryGetProperty("enum", out var enumValues) && enumValues.ValueKind == JsonValueKind.Array && enumValues.GetArrayLength() > 0)
        {
            enumValues[0].WriteTo(writer);
            return;
        }

        if (s.TryGetProperty("allOf", out var allOf) && allOf.ValueKind == JsonValueKind.Array)
        {
            var merged = new Dictionary<string, JsonElement>(StringComparer.Ordinal);
            foreach (var part in allOf.EnumerateArray())
            {
                if (Deref(part, root, 0) is { ValueKind: JsonValueKind.Object } p && p.TryGetProperty("properties", out var props) && props.ValueKind == JsonValueKind.Object)
                {
                    foreach (var property in props.EnumerateObject())
                        merged[property.Name] = property.Value;
                }
            }

            writer.WriteStartObject();
            foreach (var (name, propertySchema) in merged)
            {
                writer.WritePropertyName(name);
                WriteSample(writer, propertySchema, root, depth + 1);
            }
            writer.WriteEndObject();
            return;
        }

        foreach (var choice in new[] { "oneOf", "anyOf" })
        {
            if (s.TryGetProperty(choice, out var options) && options.ValueKind == JsonValueKind.Array && options.GetArrayLength() > 0)
            {
                WriteSample(writer, options[0], root, depth + 1);
                return;
            }
        }

        var type = s.TryGetProperty("type", out var typeValue) && typeValue.ValueKind == JsonValueKind.String ? typeValue.GetString() : null;
        if (type == "object" || (type is null && s.TryGetProperty("properties", out _)))
        {
            writer.WriteStartObject();
            if (s.TryGetProperty("properties", out var properties) && properties.ValueKind == JsonValueKind.Object)
            {
                foreach (var property in properties.EnumerateObject())
                {
                    writer.WritePropertyName(property.Name);
                    WriteSample(writer, property.Value, root, depth + 1);
                }
            }
            writer.WriteEndObject();
            return;
        }

        switch (type)
        {
            case "array":
                writer.WriteStartArray();
                if (s.TryGetProperty("items", out var items))
                    WriteSample(writer, items, root, depth + 1);
                writer.WriteEndArray();
                break;
            case "string":
                writer.WriteStringValue(StringSample(s));
                break;
            case "integer":
            case "number":
                writer.WriteNumberValue(0);
                break;
            case "boolean":
                writer.WriteBooleanValue(true);
                break;
            default:
                writer.WriteNullValue();
                break;
        }
    }

    private static string StringSample(JsonElement schema)
    {
        var format = schema.TryGetProperty("format", out var f) && f.ValueKind == JsonValueKind.String ? f.GetString() : null;
        return format switch
        {
            "date-time" => "2026-01-01T00:00:00Z",
            "date" => "2026-01-01",
            "uuid" => "00000000-0000-0000-0000-000000000000",
            "email" => "user@example.com",
            "uri" or "url" => "https://example.com",
            _ => "string",
        };
    }

    /// <summary>Follows local <c>#/…</c> references. Returns null when the reference cannot be followed.</summary>
    private static JsonElement? Deref(JsonElement node, JsonElement root, int depth)
    {
        if (depth > MaxSampleDepth || node.ValueKind != JsonValueKind.Object)
            return node.ValueKind == JsonValueKind.Undefined ? null : node;

        if (!node.TryGetProperty("$ref", out var reference) || reference.ValueKind != JsonValueKind.String)
            return node;

        var pointer = reference.GetString()!;
        if (!pointer.StartsWith("#/", StringComparison.Ordinal))
            return null;

        var current = root;
        foreach (var raw in pointer[2..].Split('/'))
        {
            var key = raw.Replace("~1", "/", StringComparison.Ordinal).Replace("~0", "~", StringComparison.Ordinal);
            if (current.ValueKind != JsonValueKind.Object || !current.TryGetProperty(key, out var next))
                return null;
            current = next;
        }

        return Deref(current, root, depth + 1);
    }
}
