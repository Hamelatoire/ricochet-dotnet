using System.Text;

namespace Ricochet.Core;

/// <summary>The outcome of sending the Request for one id.</summary>
/// <param name="StatusCode">Null when nothing was sent, or the request failed before a response came back.</param>
/// <param name="Position">The id's place in the pasted list, so a caller can tell which ids have finished when they complete out of order.</param>
public sealed record BulkItemResult(string Id, int? StatusCode, bool Succeeded, string Detail, long ElapsedMs, int Position = 0)
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
    /// The position a run resumes from: the one after <paramref name="lastId"/> in the pasted list.
    /// Zero when there is no last id, or it is no longer in the list, so the run starts from the first id.
    /// </summary>
    public static int ResumeIndex(IReadOnlyList<string> ids, string? lastId)
    {
        if (lastId is null)
            return 0;

        for (var i = 0; i < ids.Count; i++)
            if (ids[i] == lastId)
                return i + 1;

        return 0;
    }

    /// <summary>
    /// Runs the Request for each id, at most <paramref name="maxParallel"/> at a time. Ids start in the order pasted,
    /// but results arrive in completion order (see <see cref="BulkItemResult.Position"/>), one at a time.
    /// Cancelling starts no new ids; the ids in flight still finish. Returns true when every id was sent, and false when it was stopped.
    /// </summary>
    public async Task<bool> RunAsync(
        Workspace workspace,
        Collection collection,
        ApiEnvironment? environment,
        ApiRequest template,
        string placeholder,
        IReadOnlyList<string> ids,
        Action<BulkItemResult> onResult,
        CancellationToken cancellationToken = default,
        int maxParallel = 1)
    {
        if (maxParallel < 1)
            throw new ArgumentOutOfRangeException(nameof(maxParallel), "At least one thread is needed.");

        using var gate = new SemaphoreSlim(maxParallel);
        var resultLock = new object();
        var inFlight = new List<Task>();
        var stopped = false;

        try
        {
            for (var position = 0; position < ids.Count; position++)
            {
                cancellationToken.ThrowIfCancellationRequested();
                await gate.WaitAsync(cancellationToken);
                // WaitAsync can succeed even after a cancel, so the token is checked again before any id starts.
                if (cancellationToken.IsCancellationRequested)
                {
                    gate.Release();
                    throw new OperationCanceledException(cancellationToken);
                }

                var index = position;
                var id = ids[position];
                inFlight.Add(Task.Run(async () =>
                {
                    try
                    {
                        var result = (await RunOneAsync(workspace, collection, environment, template, placeholder, id)) with { Position = index };
                        lock (resultLock)
                            onResult(result);
                    }
                    finally
                    {
                        gate.Release();
                    }
                }));
            }
        }
        catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested)
        {
            stopped = true;
        }

        // Waits for the ids already started, so none is left half sent or unreported.
        await Task.WhenAll(inFlight);
        return !stopped;
    }

    private async Task<BulkItemResult> RunOneAsync(Workspace workspace, Collection collection, ApiEnvironment? environment,
        ApiRequest template, string placeholder, string id)
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
            // The token is not passed on: Stop takes effect between ids, so the id in flight still gets its response or error.
            var response = await sender.SendAsync(resolved);
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
