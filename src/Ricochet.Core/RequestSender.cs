using System.Buffers;
using System.Diagnostics;
using System.Net;
using System.Net.Security;
using System.Security.Cryptography;
using System.Text;
using System.Text.RegularExpressions;

namespace Ricochet.Core;

public sealed partial class RequestSender(HttpClient client, TimeSpan? timeout = null, long maxBodyBytes = RequestSender.DefaultMaxBodyBytes)
{
    public const long DefaultMaxBodyBytes = 100 * 1024 * 1024;

    private readonly Lock _gate = new();
    private HttpClient _client = client;
    private TimeSpan _timeout = timeout ?? TimeSpan.FromMinutes(2);
    private long _maxBodyBytes = maxBodyBytes;
    private bool _followRedirects = true;
    private bool _verifyCertificates = true;
    private CookieContainer? _cookies; // Null unless cookies are kept (ADR 0012). Held in memory only.
    private HttpClient? _clientWithoutCookies; // Used for a Request that writes its own Cookie header, so the stored cookies don't add to it.

    [GeneratedRegex(@"^[a-zA-Z][a-zA-Z0-9+.\-]*://")]
    private static partial Regex HasScheme();

    /// <summary>
    /// A client configured for an API tool: no hidden cookie jar unless <paramref name="cookies"/> is given, and our own timeout handling.
    /// Turning off certificate verification accepts any certificate, so only do it on purpose.
    /// </summary>
    public static HttpClient CreateDefaultClient(bool followRedirects = true, bool verifyCertificates = true, CookieContainer? cookies = null)
    {
        var handler = new SocketsHttpHandler
        {
            AutomaticDecompression = DecompressionMethods.All,
            AllowAutoRedirect = followRedirects,
            UseCookies = false,
            PooledConnectionLifetime = TimeSpan.FromMinutes(5),
        };
        if (cookies is not null)
        {
            // The handler stores Set-Cookie from every hop of a redirect, and sends the cookies back on each hop.
            handler.UseCookies = true;
            handler.CookieContainer = cookies;
        }
        if (!verifyCertificates)
            handler.SslOptions = new SslClientAuthenticationOptions { RemoteCertificateValidationCallback = (_, _, _, _) => true };

        return new HttpClient(handler) { Timeout = Timeout.InfiniteTimeSpan };
    }

    /// <summary>
    /// Applies new settings to the requests sent from now on. A request already in progress finishes with the
    /// settings it started with. The client it replaced is not disposed, so that request can still finish.
    /// With <paramref name="keepCookies"/>, cookies set by responses are sent on later requests from this sender (ADR 0012).
    /// Cookies already kept survive a call that keeps them on, and are dropped when it turns them off.
    /// </summary>
    public void Configure(TimeSpan timeout, bool followRedirects, bool verifyCertificates, long maxBodyBytes, bool keepCookies = false)
    {
        lock (_gate)
        {
            _timeout = timeout;
            _maxBodyBytes = maxBodyBytes;
            _followRedirects = followRedirects;
            _verifyCertificates = verifyCertificates;
            _cookies = keepCookies ? _cookies ?? new CookieContainer() : null;
            _client = CreateDefaultClient(followRedirects, verifyCertificates, _cookies);
            _clientWithoutCookies = _cookies is null ? null : CreateDefaultClient(followRedirects, verifyCertificates);
        }
    }

    /// <summary>
    /// Forgets every cookie kept by this sender, so the requests sent from now on start logged out.
    /// A request already in progress keeps the cookies it started with. Does nothing when cookies aren't kept.
    /// </summary>
    public void ResetCookies()
    {
        lock (_gate)
        {
            if (_cookies is null)
                return;

            _cookies = new CookieContainer();
            _client = CreateDefaultClient(_followRedirects, _verifyCertificates, _cookies);
        }
    }

    /// <exception cref="RequestFailedException">The request was invalid, failed, or timed out.</exception>
    /// <exception cref="OperationCanceledException">The caller cancelled.</exception>
    public async Task<ApiResponse> SendAsync(ApiRequest request, CancellationToken cancellationToken = default)
    {
        HttpClient client;
        HttpClient? clientWithoutCookies;
        TimeSpan timeout;
        long maxBodyBytes;
        lock (_gate)
        {
            client = _client;
            clientWithoutCookies = _clientWithoutCookies;
            timeout = _timeout;
            maxBodyBytes = _maxBodyBytes;
        }

        using var timeoutCts = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        timeoutCts.CancelAfter(timeout);

        try
        {
            var stopwatch = Stopwatch.StartNew();
            using var response = await SendWithAuthAsync(client, clientWithoutCookies, request, timeoutCts.Token);
            var (body, truncated) = await ReadBodyAsync(response.Content, maxBodyBytes, timeoutCts.Token);
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
            throw new RequestFailedException($"Timed out after {timeout.TotalSeconds:0} seconds.", ex);
        }
        catch (Exception ex) when (ex is HttpRequestException or IOException or FormatException or ArgumentException)
        {
            throw new RequestFailedException(ex.Message, ex);
        }
    }

    /// <summary>
    /// Sends the request. Digest needs the server's challenge first, so a 401 with a Digest challenge is answered with one
    /// more send of the same request (ADR 0024). The body is built again, so it is sent in full both times.
    /// </summary>
    private static async Task<HttpResponseMessage> SendWithAuthAsync(HttpClient client, HttpClient? clientWithoutCookies, ApiRequest request, CancellationToken cancellationToken)
    {
        using var message = BuildMessage(request, null);
        // A Request that writes its own Cookie header is sent without the stored cookies (ADR 0012).
        var sendingClient = clientWithoutCookies is not null && message.Headers.Contains("Cookie") ? clientWithoutCookies : client;
        var response = await sendingClient.SendAsync(message, HttpCompletionOption.ResponseHeadersRead, cancellationToken);
        if (request.Auth?.Kind != AuthKinds.Digest || response.StatusCode != HttpStatusCode.Unauthorized)
            return response;

        var challenge = DigestChallenge(response);
        if (challenge is null)
            return response;

        response.Dispose();
        using var retry = BuildMessage(request, challenge);
        var retryClient = clientWithoutCookies is not null && retry.Headers.Contains("Cookie") ? clientWithoutCookies : client;
        return await retryClient.SendAsync(retry, HttpCompletionOption.ResponseHeadersRead, cancellationToken);
    }

    /// <summary>The text after the "Digest" scheme of a WWW-Authenticate header, or null when there is none.</summary>
    private static string? DigestChallenge(HttpResponseMessage response)
    {
        if (!response.Headers.TryGetValues("WWW-Authenticate", out var values))
            return null;

        foreach (var value in values)
        {
            var text = value.TrimStart();
            if (text.StartsWith("Digest ", StringComparison.OrdinalIgnoreCase))
                return text[7..];
        }

        return null;
    }

    private static HttpRequestMessage BuildMessage(ApiRequest request, string? digestChallenge)
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

        ApplyAuth(message, request, digestChallenge);
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

    /// <summary>Adds the headers for the request's auth kind. Digest adds its header only once the server has sent a challenge.</summary>
    private static void ApplyAuth(HttpRequestMessage message, ApiRequest request, string? digestChallenge)
    {
        var auth = request.Auth;
        if (auth is null)
            return;

        var uri = message.RequestUri!;
        switch (auth.Kind)
        {
            case AuthKinds.Bearer:
                AddAuthHeader(message, "Authorization", $"Bearer {auth.Token}");
                break;
            case AuthKinds.Basic:
                AddAuthHeader(message, "Authorization", "Basic " + Convert.ToBase64String(Encoding.UTF8.GetBytes($"{auth.Username}:{auth.Password}")));
                break;
            case AuthKinds.ApiKey when !auth.KeyInQuery && auth.KeyName.Length > 0:
                AddAuthHeader(message, auth.KeyName, auth.KeyValue);
                break;
            case AuthKinds.Digest when digestChallenge is not null:
                AddAuthHeader(message, "Authorization",
                    AuthSigning.DigestAuthorization(auth, message.Method.Method, uri.PathAndQuery, digestChallenge, RandomHex()));
                break;
            case AuthKinds.OAuth1:
                List<KeyValuePair<string, string>> form = message.Content?.Headers.ContentType?.MediaType is "application/x-www-form-urlencoded"
                    ? AuthSigning.ParseQuery(request.Body)
                    : [];
                AddAuthHeader(message, "Authorization",
                    AuthSigning.OAuth1Authorization(auth, message.Method.Method, uri, form, RandomHex(), DateTimeOffset.UtcNow.ToUnixTimeSeconds()));
                break;
            case AuthKinds.AwsSigV4:
                foreach (var header in AuthSigning.AwsSigV4Headers(auth, message.Method.Method, uri, Encoding.UTF8.GetBytes(request.Body), DateTimeOffset.UtcNow))
                    AddAuthHeader(message, header.Key, header.Value);
                break;
        }
    }

    /// <summary>A header the user wrote explicitly wins over the request's auth settings.</summary>
    private static void AddAuthHeader(HttpRequestMessage message, string name, string value)
    {
        if (name.Length == 0 || message.Headers.Contains(name))
            return;

        if (!message.Headers.TryAddWithoutValidation(name, value))
            throw new RequestFailedException($"Invalid auth header: \"{name}\".");
    }

    private static string RandomHex() => Convert.ToHexString(RandomNumberGenerator.GetBytes(16)).ToLowerInvariant();

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

    internal static string GuessContentType(string body)
    {
        var start = body.AsSpan().TrimStart();
        return start.Length > 0 && start[0] is '{' or '[' ? "application/json" : "text/plain; charset=utf-8";
    }

    private static async Task<(byte[] Body, bool Truncated)> ReadBodyAsync(HttpContent content, long maxBodyBytes, CancellationToken cancellationToken)
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
