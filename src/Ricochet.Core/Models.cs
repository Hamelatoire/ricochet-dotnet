using System.Text.Json.Serialization;

namespace Ricochet.Core;

// Persisted records use `set`, not `init`: the JSON source generator nulls out init-only
// properties that are missing from a file, which would bypass the defaults declared here.

public sealed record KeyValue(
    string Key,
    string Value,
    bool Enabled = true,
    [property: JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingDefault)] bool IsSecret = false);

public static class AuthKinds
{
    public const string None = "none";
    public const string Bearer = "bearer";
    public const string Basic = "basic";
    public const string ApiKey = "apikey";
}

public sealed record RequestAuth
{
    public string Kind { get; set; } = AuthKinds.None;
    public string Token { get; set; } = "";
    public string Username { get; set; } = "";
    public string Password { get; set; } = "";
    public string KeyName { get; set; } = "";
    public string KeyValue { get; set; } = "";
    public bool KeyInQuery { get; set; }
}

public sealed record ApiRequest
{
    public string Id { get; set; } = Guid.NewGuid().ToString("N");
    public string Name { get; set; } = "";
    public string Method { get; set; } = "GET";
    public string Url { get; set; } = "";
    public IReadOnlyList<KeyValue> Headers { get; set; } = [];
    public string Body { get; set; } = "";
    public RequestAuth? Auth { get; set; }

    /// <summary>Null rather than empty when there are none, so they add nothing to the Request's file.</summary>
    public IReadOnlyList<RequestAssertion>? Assertions { get; set; }

    /// <summary>Null rather than empty when there are none, for the same reason.</summary>
    public IReadOnlyList<RequestCapture>? Captures { get; set; }

    /// <summary>Set on Requests made from an OpenAPI description: the operationId, or "METHOD /path" when there is none.</summary>
    public string? SpecKey { get; set; }
}

public sealed record ApiResponse(
    int StatusCode,
    string ReasonPhrase,
    IReadOnlyList<KeyValue> Headers,
    byte[] Body,
    string? MediaType,
    string? CharSet,
    TimeSpan Elapsed,
    bool Truncated);

/// <summary>A script kept from an import. It is stored but never run.</summary>
public sealed record StoredScript(string Listen, string Source);

/// <summary>A Folder when <see cref="Request"/> is null, otherwise a Request.</summary>
public sealed record CollectionItem
{
    public string Id { get; set; } = Guid.NewGuid().ToString("N");
    public string Name { get; set; } = "";
    public ApiRequest? Request { get; set; }
    public IReadOnlyList<CollectionItem> Children { get; set; } = [];
    public IReadOnlyList<StoredScript> Scripts { get; set; } = [];
}

public sealed record Collection
{
    public string Id { get; set; } = Guid.NewGuid().ToString("N");
    public string Name { get; set; } = "";
    public IReadOnlyList<CollectionItem> Items { get; set; } = [];
    public IReadOnlyList<KeyValue> Variables { get; set; } = [];
    public IReadOnlyList<StoredScript> Scripts { get; set; } = [];

    /// <summary>Set on a Collection kept up to date from an OpenAPI URL (ADR 0014). Null otherwise.</summary>
    public string? SpecUrl { get; set; }

    /// <summary>Folders and requests the user has hidden (ADR 0032). They stay in Items, and are skipped by runs.</summary>
    public IReadOnlyList<string>? HiddenItemIds { get; set; }
}

public sealed record ApiEnvironment
{
    public string Id { get; set; } = Guid.NewGuid().ToString("N");
    public string Name { get; set; } = "";
    public IReadOnlyList<KeyValue> Variables { get; set; } = [];
}

public sealed record HistoryEntry(ApiRequest Request, int? StatusCode, long ElapsedMs, DateTimeOffset At, string? CollectionId = null);

public sealed record Workspace
{
    public const int MaxHistory = 200;
    public const string DefaultCollectionName = "My Requests";

    public IReadOnlyList<Collection> Collections { get; set; } = [];
    public IReadOnlyList<ApiEnvironment> Environments { get; set; } = [];
    public string? ActiveEnvironmentId { get; set; }
    public IReadOnlyList<KeyValue> Globals { get; set; } = [];
    public IReadOnlyList<HistoryEntry> History { get; set; } = [];

    /// <summary>From the original file format. Read so it can be migrated, never written.</summary>
    [JsonPropertyName("saved"), JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public IReadOnlyList<ApiRequest>? LegacySaved { get; set; }

    /// <summary>From the original file format. Read so it can be migrated, never written.</summary>
    [JsonPropertyName("variables"), JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)]
    public IReadOnlyList<KeyValue>? LegacyVariables { get; set; }

    public Workspace MigrateLegacyFields()
    {
        if (LegacySaved is null && LegacyVariables is null)
            return this;

        var collections = Collections;
        if (LegacySaved is { Count: > 0 })
        {
            var migrated = new Collection
            {
                Name = DefaultCollectionName,
                Items = LegacySaved.Select(r => new CollectionItem { Name = r.Name, Request = r }).ToList(),
            };
            collections = [.. Collections, migrated];
        }

        return this with
        {
            Collections = collections,
            Globals = LegacyVariables is { Count: > 0 } ? LegacyVariables : Globals,
            LegacySaved = null,
            LegacyVariables = null,
        };
    }
}

/// <summary>A request that could not complete. The message is suitable for showing to the user.</summary>
public sealed class RequestFailedException(string message, Exception? inner = null) : Exception(message, inner);
