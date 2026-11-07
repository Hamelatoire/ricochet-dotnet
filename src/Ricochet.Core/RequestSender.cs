using System.Buffers;
using System.Diagnostics;
using System.Net;
using System.Net.Security;
using System.Security.Cryptography;
using System.Security.Cryptography.X509Certificates;
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

    /// <summary>Set when the proxy or client certificate settings cannot be used. Every request fails with it until they change.</summary>
    private string? _configurationProblem;

    [GeneratedRegex(@"^[a-zA-Z][a-zA-Z0-9+.\-]*://")]
    private static partial Regex HasScheme();

    /// <summary>
    /// A client configured for an API tool: no hidden cookie jar, and our own timeout handling.
    /// Turning off certificate verification accepts any certificate, so only do it on purpose.
    /// </summary>
    public static HttpClient CreateDefaultClient(bool followRedirects = true, bool verifyCertificates = true, Uri? proxy = null, X509Certificate2? clientCertificate = null)
    {
        var handler = new SocketsHttpHandler
        {
            AutomaticDecompression = DecompressionMethods.All,
            AllowAutoRedirect = followRedirects,
            UseCookies = false,
            PooledConnectionLifetime = TimeSpan.FromMinutes(5),
        };
        if (proxy is not null)
        {
            handler.UseProxy = true;
            handler.Proxy = new WebProxy(proxy);
        }

        if (!verifyCertificates || clientCertificate is not null)
        {
            var ssl = new SslClientAuthenticationOptions();
            if (!verifyCertificates)
                ssl.RemoteCertificateValidationCallback = (_, _, _, _) => true;
            if (clientCertificate is not null)
                ssl.ClientCertificates = new X509CertificateCollection { clientCertificate };
            handler.SslOptions = ssl;
        }

        return new HttpClient(handler) { Timeout = Timeout.InfiniteTimeSpan };
    }

    /// <summary>
    /// Applies new settings to the requests sent from now on. A request already in progress finishes with the
    /// settings it started with. The client it replaced is not disposed, so that request can still finish.
    /// Returns the problem with the proxy or client certificate settings, to show beside them, or null when they are usable.
    /// An empty proxy uses the operating system's proxy, and an empty certificate path sends no certificate (ADR 0026).
    /// </summary>
    public string? Configure(
        TimeSpan timeout,
        bool followRedirects,
        bool verifyCertificates,
        long maxBodyBytes,
        string proxyUrl = "",
        string clientCertificatePath = "",
        string? clientCertificatePassword = null)
    {
        string? problem = null;
        Uri? proxy = null;
        X509Certificate2? certificate = null;

        try
        {
            proxy = ParseProxy(proxyUrl);
            if (clientCertificatePath.Trim().Length > 0)
                certificate = LoadClientCertificate(clientCertificatePath, clientCertificatePassword);
        }
        catch (RequestFailedException ex)
        {
            problem = ex.Message;
        }

        lock (_gate)
        {
            _timeout = timeout;
            _maxBodyBytes = maxBodyBytes;
            _configurationProblem = problem;
            _client = CreateDefaultClient(followRedirects, verifyCertificates, proxy, certificate);
        }

        return problem;
    }

    /// <summary>
    /// Reads a proxy address such as "proxy.example.com:8080". Empty means no explicit proxy. Credentials in the address are refused,
    /// because they would be kept in plain text, and proxy logins are not supported yet.
    /// </summary>
    /// <exception cref="RequestFailedException">The address is not an http or https address.</exception>
    public static Uri? ParseProxy(string text)
    {
        var value = text.Trim();
        if (value.Length == 0)
            return null;

        if (!HasScheme().IsMatch(value))
            value = "http://" + value;

        if (!Uri.TryCreate(value, UriKind.Absolute, out var uri) || uri.Scheme is not ("http" or "https") || uri.Host.Length == 0)
            throw new RequestFailedException($"Invalid proxy address: {text.Trim()}. Use a form such as http://proxy.example.com:8080.");

        if (uri.UserInfo.Length > 0)
            throw new RequestFailedException("Proxy logins are not supported yet. Remove the user name and password from the proxy address.");

        return uri;
    }

    /// <summary>Loads a client certificate from a PKCS #12 file (.pfx or .p12). The password is kept only in memory.</summary>
    /// <exception cref="RequestFailedException">The file is missing, or cannot be read with this password.</exception>
    public static X509Certificate2 LoadClientCertificate(string path, string? password)
    {
        var fullPath = path.Trim();
        if (!File.Exists(fullPath))
            throw new RequestFailedException($"The client certificate file was not found: {fullPath}");

        try
        {
            return X509CertificateLoader.LoadPkcs12FromFile(fullPath, string.IsNullOrEmpty(password) ? null : password);
        }
        catch (Exception ex) when (ex is CryptographicException or IOException or UnauthorizedAccessException)
        {
            throw new RequestFailedException($"The client certificate could not be loaded from {fullPath}: {ex.Message}", ex);
        }
    }

    /// <exception cref="RequestFailedException">The request was invalid, failed, or timed out.</exception>
    /// <exception cref="OperationCanceledException">The caller cancelled.</exception>
    public async Task<ApiResponse> SendAsync(ApiRequest request, CancellationToken cancellationToken = default)
    {
        HttpClient client;
        TimeSpan timeout;
        long maxBodyBytes;
        string? problem;
        lock (_gate)
        {
            client = _client;
            timeout = _timeout;
            maxBodyBytes = _maxBodyBytes;
            problem = _configurationProblem;
        }

        if (problem is not null)
            throw new RequestFailedException(problem);

        using var timeoutCts = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        timeoutCts.CancelAfter(timeout);

        try
        {
            using var message = BuildMessage(request);
            var stopwatch = Stopwatch.StartNew();
            using var response = await client.SendAsync(message, HttpCompletionOption.ResponseHeadersRead, timeoutCts.Token);
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
