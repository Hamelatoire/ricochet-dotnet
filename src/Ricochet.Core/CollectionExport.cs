using System.Text;

namespace Ricochet.Core;

/// <summary>Something an export left out or replaced. Shown to the user after every export, as imports are (ADR 0002).</summary>
public sealed record ExportNote(string Category, string Location, string Detail);

public static class ExportCategories
{
    public const string SecretVariables = "Secret Variables left out";
    public const string SecretValues = "Secret values kept out of the export";
    public const string NotExported = "Not exported";
}

/// <summary>The text of an export, and a note for everything that was left out or replaced.</summary>
public sealed record ExportResult(string Json, IReadOnlyList<ExportNote> Notes)
{
    public string ToReportText()
    {
        var text = new StringBuilder();
        foreach (var group in Notes.GroupBy(n => n.Category))
        {
            text.AppendLine($"{group.Key} ({group.Count()}):");
            foreach (var note in group)
                text.AppendLine(note.Detail.Length > 0 ? $"  {note.Location}: {note.Detail}" : $"  {note.Location}");
            text.AppendLine();
        }

        return text.ToString().TrimEnd();
    }
}

/// <summary>
/// Shared by the exporters. Secret values (ADR 0004) never reach an export: Secret Variables are left out,
/// and any other text that contains their value has it replaced with the Variable's placeholder.
/// </summary>
internal sealed class ExportContext
{
    private readonly List<(string Name, string Value)> _secretValues = [];

    public ExportContext(Collection collection)
    {
        foreach (var variable in collection.Variables.Where(v => v.IsSecret))
        {
            var empty = variable.Value.Length == 0 ? " (has no value on this machine)" : "";
            Notes.Add(new ExportNote(ExportCategories.SecretVariables, collection.Name, variable.Key + empty));
            if (variable.Value.Length > 0)
                _secretValues.Add((variable.Key, variable.Value));
        }

        // Longest first, so a value that contains another is replaced whole.
        _secretValues.Sort((a, b) => b.Value.Length.CompareTo(a.Value.Length));
    }

    public List<ExportNote> Notes { get; } = [];

    public static string Placeholder(string name) => "{{" + name + "}}";

    public static string Plural(int count, string noun) => count == 1 ? $"1 {noun}" : $"{count} {noun}s";

    /// <summary>Returns the text with every known secret value replaced by its placeholder.</summary>
    public string Scrub(string text, string location)
    {
        foreach (var (name, value) in _secretValues)
        {
            if (!text.Contains(value, StringComparison.Ordinal))
                continue;

            text = text.Replace(value, Placeholder(name), StringComparison.Ordinal);
            Notes.Add(new ExportNote(ExportCategories.SecretValues, location, $"the value of secret {name} was replaced with {Placeholder(name)}"));
        }

        return text;
    }
}

/// <summary>A value typed directly into a Request, such as a bearer token (ADR 0004), and the placeholder written instead.</summary>
internal sealed record TypedSecret(string Field, string Placeholder);

/// <summary>A Request with its secret values already replaced, ready for either export format.</summary>
internal sealed record PreparedRequest(string Method, string Url, List<KeyValue> Headers, string Body, RequestAuth Auth, List<TypedSecret> TypedSecrets)
{
    public static PreparedRequest Create(ExportContext context, ApiRequest request, string location)
    {
        var typed = new List<TypedSecret>();

        string Literal(string value, string placeholderName, string field)
        {
            var placeholder = ExportContext.Placeholder(placeholderName);
            typed.Add(new TypedSecret(field, placeholder));
            return placeholder;
        }

        var headers = new List<KeyValue>();
        foreach (var header in request.Headers.Where(h => h.Key.Trim().Length > 0))
        {
            var name = header.Key.Trim();
            var value = SecretRules.IsSensitiveHeader(name) && SecretRules.IsLiteral(header.Value)
                ? Literal(header.Value, Clean(name, "header"), $"header {name}")
                : context.Scrub(header.Value, location);
            headers.Add(new KeyValue(name, value, header.Enabled));
        }

        var source = request.Auth ?? new RequestAuth();
        var auth = source.Kind switch
        {
            AuthKinds.Bearer => new RequestAuth
            {
                Kind = AuthKinds.Bearer,
                Token = SecretRules.IsLiteral(source.Token) ? Literal(source.Token, "token", "bearer token") : context.Scrub(source.Token, location),
            },
            AuthKinds.Basic => new RequestAuth
            {
                Kind = AuthKinds.Basic,
                Username = context.Scrub(source.Username, location),
                Password = SecretRules.IsLiteral(source.Password) ? Literal(source.Password, "password", "basic password") : context.Scrub(source.Password, location),
            },
            AuthKinds.ApiKey => new RequestAuth
            {
                Kind = AuthKinds.ApiKey,
                KeyName = context.Scrub(source.KeyName, location),
                KeyValue = SecretRules.IsLiteral(source.KeyValue)
                    ? Literal(source.KeyValue, Clean(source.KeyName, "apiKey"), $"API key {source.KeyName}")
                    : context.Scrub(source.KeyValue, location),
                KeyInQuery = source.KeyInQuery,
            },
            _ => new RequestAuth(),
        };

        return new PreparedRequest(
            request.Method.ToUpperInvariant(),
            context.Scrub(request.Url, location),
            headers,
            context.Scrub(request.Body, location + " body"),
            auth,
            typed);
    }

    public static string? ContentType(IEnumerable<KeyValue> headers) =>
        headers.FirstOrDefault(h => h.Enabled && h.Key.Equals("Content-Type", StringComparison.OrdinalIgnoreCase))?.Value;

    /// <summary>A name that can stand inside {{ }} and as a parameter name, or the fallback.</summary>
    public static string Clean(string name, string fallback) =>
        name.Length > 0 && name.All(c => char.IsAsciiLetterOrDigit(c) || c is '_' or '-' or '.') ? name : fallback;
}

internal static class JsonHelpers
{
    /// <summary>Adds through the non-generic interface method, which needs no runtime code generation under Native AOT.</summary>
    public static void Append(System.Text.Json.Nodes.JsonArray array, System.Text.Json.Nodes.JsonNode? node) =>
        ((ICollection<System.Text.Json.Nodes.JsonNode?>)array).Add(node);
}
