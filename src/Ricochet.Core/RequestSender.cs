using System.Buffers;
using System.Diagnostics;
using System.Net;
using System.Text;
using System.Text.RegularExpressions;

namespace Ricochet.Core;

public sealed partial class RequestSender(HttpClient client, TimeSpan? timeout = null, long maxBodyBytes = RequestSender.DefaultMaxBodyBytes)
{
    public const long DefaultMaxBodyBytes = 100 * 1024 * 1024;

    private readonly TimeSpan _timeout = timeout ?? TimeSpan.FromMinutes(2);

    [GeneratedRegex(@"^[a-zA-Z][a-zA-Z0-9+.\-]*://")]
    private static partial Regex HasScheme();

    /// <summary>A client configured for an API tool: no hidden cookie jar, and our own timeout handling.</summary>
    public static HttpClient CreateDefaultClient() => new(new SocketsHttpHandler
    {
        AutomaticDecompression = DecompressionMethods.All,
        AllowAutoRedirect = true,
        UseCookies = false,
        PooledConnectionLifetime = TimeSpan.FromMinutes(5),
    })
    {
        Timeout = Timeout.InfiniteTimeSpan,
    };

    /// <exception cref="RequestFailedException">The request was invalid, failed, or timed out.</exception>
    /// <exception cref="OperationCanceledException">The caller cancelled.</exception>
    public async Task<ApiResponse> SendAsync(ApiRequest request, CancellationToken cancellationToken = default)
    {
        using var timeoutCts = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        timeoutCts.CancelAfter(_timeout);

        try
        {
            using var message = BuildMessage(request);
            var stopwatch = Stopwatch.StartNew();
            using var response = await client.SendAsync(message, HttpCompletionOption.ResponseHeadersRead, timeoutCts.Token);
            var (body, truncated) = await ReadBodyAsync(response.Content, timeoutCts.Token);
            stopwatch.Stop();

            return new ApiResponse(
                (int)response.StatusCode,
                response.ReasonPhrase ?? "",
                CollectHeaders(response),
                body,
                response.Content.Headers.ContentType?.MediaType,
                response.Content.Headers.ContentType?.CharSet,
                stopwatch.Elapsed,
                truncated);
        }
        catch (OperationCanceledException ex) when (!cancellationToken.IsCancellationRequested)
        {
            throw new RequestFailedException($"Timed out after {_timeout.TotalSeconds:0} seconds.", ex);
        }
        catch (Exception ex) when (ex is HttpRequestException or IOException or FormatException or ArgumentException)
        {
            throw new RequestFailedException(ex.Message, ex);
        }
    }

    private static HttpRequestMessage BuildMessage(ApiRequest request)
    {
        var message = new HttpRequestMessage(new HttpMethod(request.Method), ApplyQueryAuth(ParseUrl(request.Url), request.Auth));
        var headers = request.Headers.Where(h => h.Enabled && h.Key.Trim().Length > 0).ToList();

        if (request.Body.Length > 0)
        {
            message.Content = new ByteArrayContent(Encoding.UTF8.GetBytes(request.Body));
            var hasContentType = headers.Any(h => h.Key.Trim().Equals("Content-Type", StringComparison.OrdinalIgnoreCase));
            if (!hasContentType)
                message.Content.Headers.TryAddWithoutValidation("Content-Type", GuessContentType(request.Body));
        }

        foreach (var header in headers)
        {
            var key = header.Key.Trim();
            if (message.Headers.TryAddWithoutValidation(key, header.Value))
                continue;

            var isContentHeader = key.StartsWith("Content-", StringComparison.OrdinalIgnoreCase);
            if (isContentHeader && message.Content is null)
                continue; // Content headers have nothing to describe without a body.

            if (message.Content is null || !message.Content.Headers.TryAddWithoutValidation(key, header.Value))
                throw new RequestFailedException($"Invalid header: \"{key}\".");
        }

        ApplyHeaderAuth(message, request.Auth);
        return message;
    }

    private static Uri ApplyQueryAuth(Uri uri, RequestAuth? auth)
    {
        if (auth is not { Kind: AuthKinds.ApiKey, KeyInQuery: true } || auth.KeyName.Length == 0)
            return uri;

        var pair = $"{Uri.EscapeDataString(auth.KeyName)}={Uri.EscapeDataString(auth.KeyValue)}";
        var builder = new UriBuilder(uri);
        builder.Query = builder.Query.Length > 1 ? $"{builder.Query[1..]}&{pair}" : pair;
        return builder.Uri;
    }

    /// <summary>A header the user wrote explicitly wins over the request's auth settings.</summary>
    private static void ApplyHeaderAuth(HttpRequestMessage message, RequestAuth? auth)
    {
        var (name, value) = auth?.Kind switch
        {
            AuthKinds.Bearer => ("Authorization", $"Bearer {auth.Token}"),
            AuthKinds.Basic => ("Authorization", "Basic " + Convert.ToBase64String(Encoding.UTF8.GetBytes($"{auth.Username}:{auth.Password}"))),
            AuthKinds.ApiKey when !auth.KeyInQuery && auth.KeyName.Length > 0 => (auth.KeyName, auth.KeyValue),
            _ => ("", ""),
        };

        if (name.Length == 0 || message.Headers.Contains(name))
            return;

        if (!message.Headers.TryAddWithoutValidation(name, value))
            throw new RequestFailedException($"Invalid auth header: \"{name}\".");
    }

    private static Uri ParseUrl(string rawUrl)
    {
        var url = rawUrl.Trim();
        if (url.Length == 0)
            throw new RequestFailedException("Enter a URL.");

        if (!HasScheme().IsMatch(url))
            url = "http://" + url;

        if (!Uri.TryCreate(url, UriKind.Absolute, out var uri) || uri.Scheme is not ("http" or "https"))
            throw new RequestFailedException($"Invalid URL: {rawUrl.Trim()}");

        return uri;
    }

    private static string GuessContentType(string body)
    {
        var start = body.AsSpan().TrimStart();
        return start.Length > 0 && start[0] is '{' or '[' ? "application/json" : "text/plain; charset=utf-8";
    }

    private async Task<(byte[] Body, bool Truncated)> ReadBodyAsync(HttpContent content, CancellationToken cancellationToken)
    {
        await using var stream = await content.ReadAsStreamAsync(cancellationToken);
        using var buffer = new MemoryStream();
        var chunk = ArrayPool<byte>.Shared.Rent(81920);
        try
        {
            while (true)
            {
                var read = await stream.ReadAsync(chunk, cancellationToken);
                if (read == 0)
                    return (buffer.ToArray(), false);

                var remaining = maxBodyBytes - buffer.Length;
                if (read > remaining)
                {
                    buffer.Write(chunk, 0, (int)remaining);
                    return (buffer.ToArray(), true);
                }

                buffer.Write(chunk, 0, read);
            }
        }
        finally
        {
            ArrayPool<byte>.Shared.Return(chunk);
        }
    }

    private static List<KeyValue> CollectHeaders(HttpResponseMessage response)
    {
        var headers = new List<KeyValue>();
        foreach (var (key, values) in response.Headers.Concat(response.Content.Headers))
        {
            foreach (var value in values)
                headers.Add(new KeyValue(key, value));
        }

        return headers;
    }
}
