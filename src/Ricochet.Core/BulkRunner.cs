using System.Text;

namespace Ricochet.Core;

/// <summary>The outcome of sending the Request for one id.</summary>
/// <param name="StatusCode">Null when nothing was sent, or the request failed before a response came back.</param>
public sealed record BulkItemResult(string Id, int? StatusCode, bool Succeeded, string Detail, long ElapsedMs)
{
    /// <summary>One line for a log: OK or ERR, the id, the status and the response start.</summary>
    public string LogLine => $"{(Succeeded ? "OK " : "ERR")} {Id}  [{StatusCode?.ToString() ?? "-"}] {Detail}".TrimEnd();
}

/// <summary>
/// Sends one Request for each id, one at a time (for example, a request whose URL contains {{id}}).
/// Nothing here stores progress: the caller records each result as it arrives.
/// </summary>
public sealed class BulkRunner(RequestSender sender)
{
    public const string DefaultPlaceholder = "id";

    private const int MaxDetailLength = 300;

    /// <summary>Splits pasted text into ids: one per line, with blank lines and surrounding spaces dropped.</summary>
    public static IReadOnlyList<string> ParseIds(string text) =>
        text.Split(['\r', '\n'], StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries)
            .Where(id => id.Length > 0)
            .ToList();

    /// <summary>
    /// Runs the Request for each id in order, stopping before the next id when cancelled.
    /// Returns true when every id was sent, and false when it was stopped.
    /// </summary>
    public async Task<bool> RunAsync(
        Workspace workspace,
        Collection collection,
        ApiEnvironment? environment,
        ApiRequest template,
        string placeholder,
        IReadOnlyList<string> ids,
        Action<BulkItemResult> onResult,
        CancellationToken cancellationToken = default)
    {
        try
        {
            foreach (var id in ids)
            {
                cancellationToken.ThrowIfCancellationRequested();
                onResult(await RunOneAsync(workspace, collection, environment, template, placeholder, id, cancellationToken));
            }
            return true;
        }
        catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested)
        {
            return false;
        }
    }

    private async Task<BulkItemResult> RunOneAsync(Workspace workspace, Collection collection, ApiEnvironment? environment,
        ApiRequest template, string placeholder, string id, CancellationToken cancellationToken)
    {
        var variables = VariableResolver.Layer(workspace.Globals, collection.Variables, environment?.Variables, [new KeyValue(placeholder, id)]);
        var resolved = VariableResolver.Apply(template, variables);
        var secrets = CollectionRunner.SecretsToMask(variables, [], [], resolved);

        // Sending a literal {{name}} could reach the wrong place, so an unresolved one stops this id only.
        var missing = VariableResolver.FindPlaceholders(resolved);
        if (missing.Count > 0)
            return new BulkItemResult(id, null, false, $"Not sent, because there is no value for: {string.Join(", ", missing)}", 0);

        try
        {
            var response = await sender.SendAsync(resolved, cancellationToken);
            var succeeded = response.StatusCode is >= 200 and < 300;
            var detail = succeeded ? "" : Snippet(response.Body);
            return new BulkItemResult(id, response.StatusCode, succeeded, CollectionRunner.MaskIn(detail, secrets),
                (long)response.Elapsed.TotalMilliseconds);
        }
        catch (RequestFailedException ex)
        {
            return new BulkItemResult(id, null, false, CollectionRunner.MaskIn(ex.Message, secrets), 0);
        }
    }

    private static string Snippet(byte[] body)
    {
        var text = Encoding.UTF8.GetString(body).ReplaceLineEndings(" ").Trim();
        return text.Length <= MaxDetailLength ? text : text[..MaxDetailLength] + "…";
    }
}
