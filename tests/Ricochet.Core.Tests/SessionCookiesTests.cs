using System.Net;
using System.Net.Sockets;
using System.Text;
using Ricochet.Core;

namespace Ricochet.Core.Tests;

/// <summary>
/// Cookie behaviour needs the real SocketsHttpHandler (its cookie handling and redirects can't be faked with an
/// HttpMessageHandler), so these tests use a small HTTP server on the loopback address. It accepts no outside traffic.
/// </summary>
public class SessionCookiesTests
{
    private sealed record Seen(string Path, string? Cookie);

    private sealed record Reply(int Status, string[] Headers, string Body = "");

    private sealed class LoopbackServer : IDisposable
    {
        private readonly TcpListener _listener = new(IPAddress.Loopback, 0);
        private readonly Func<Seen, Reply> _respond;
        private readonly CancellationTokenSource _stop = new();
        private readonly List<Seen> _seen = [];

        public LoopbackServer(Func<Seen, Reply> respond)
        {
            _respond = respond;
            _listener.Start();
            BaseUri = new Uri($"http://127.0.0.1:{((IPEndPoint)_listener.LocalEndpoint).Port}");
            _ = Task.Run(AcceptLoopAsync);
        }

        public Uri BaseUri { get; }

        public List<Seen> Seen
        {
            get
            {
                lock (_seen)
                    return [.. _seen];
            }
        }

        private async Task AcceptLoopAsync()
        {
            while (!_stop.IsCancellationRequested)
            {
                try
                {
                    var client = await _listener.AcceptTcpClientAsync(_stop.Token);
                    _ = Task.Run(() => HandleAsync(client));
                }
                catch (OperationCanceledException)
                {
                    return;
                }
                catch (SocketException)
                {
                    return;
                }
            }
        }

        private async Task HandleAsync(TcpClient client)
        {
            using (client)
            {
                var stream = client.GetStream();
                var reader = new StreamReader(stream, Encoding.ASCII);
                var requestLine = await reader.ReadLineAsync() ?? "";
                string? cookie = null;
                string? line;
                while (!string.IsNullOrEmpty(line = await reader.ReadLineAsync()))
                {
                    if (line.StartsWith("Cookie:", StringComparison.OrdinalIgnoreCase))
                        cookie = line["Cookie:".Length..].Trim();
                }

                var target = requestLine.Split(' ')[1];
                var seen = new Seen(target, cookie);
                lock (_seen)
                    _seen.Add(seen);

                var reply = _respond(seen);
                var sb = new StringBuilder();
                sb.Append($"HTTP/1.1 {reply.Status} Reason\r\n");
                foreach (var header in reply.Headers)
                    sb.Append(header).Append("\r\n");
                sb.Append($"Content-Length: {Encoding.UTF8.GetByteCount(reply.Body)}\r\n");
                sb.Append("Connection: close\r\n\r\n");
                sb.Append(reply.Body);

                var bytes = Encoding.UTF8.GetBytes(sb.ToString());
                await stream.WriteAsync(bytes);
                await stream.FlushAsync();
            }
        }

        public void Dispose()
        {
            _stop.Cancel();
            _listener.Stop();
            _stop.Dispose();
        }
    }

    /// <summary>Logs in at /login (setting a session cookie), redirects /redirect-login with a cookie, and records everything else.</summary>
    private static Reply Site(Seen seen) => seen.Path switch
    {
        "/login" => new Reply(200, ["Set-Cookie: session=abc123; Path=/"], "ok"),
        "/redirect-login" => new Reply(302, ["Set-Cookie: session=from-redirect; Path=/", "Location: /me"]),
        _ => new Reply(200, [], "hello"),
    };

    private static RequestSender Sender(bool keepCookies)
    {
        var sender = new RequestSender(new HttpClient());
        sender.Configure(TimeSpan.FromSeconds(10), followRedirects: true, verifyCertificates: true, RequestSender.DefaultMaxBodyBytes, keepCookies: keepCookies);
        return sender;
    }

    private static ApiRequest Get(Uri baseUri, string path, params KeyValue[] headers) => new()
    {
        Name = path,
        Method = "GET",
        Url = new Uri(baseUri, path).ToString(),
        Headers = [.. headers],
    };

    [Fact]
    public async Task A_cookie_set_by_one_response_is_sent_on_the_next_request()
    {
        using var server = new LoopbackServer(Site);
        var sender = Sender(keepCookies: true);

        await sender.SendAsync(Get(server.BaseUri, "/login"), TestContext.Current.CancellationToken);
        await sender.SendAsync(Get(server.BaseUri, "/me"), TestContext.Current.CancellationToken);

        Assert.Null(server.Seen[0].Cookie);
        Assert.Equal("session=abc123", server.Seen[1].Cookie);
    }

    [Fact]
    public async Task A_cookie_set_on_a_redirect_is_sent_on_the_next_request()
    {
        using var server = new LoopbackServer(Site);
        var sender = Sender(keepCookies: true);

        await sender.SendAsync(Get(server.BaseUri, "/redirect-login"), TestContext.Current.CancellationToken);
        await sender.SendAsync(Get(server.BaseUri, "/later"), TestContext.Current.CancellationToken);

        Assert.Equal("session=from-redirect", server.Seen.Last().Cookie);
    }

    [Fact]
    public async Task Cookies_are_not_sent_when_keeping_them_is_off()
    {
        using var server = new LoopbackServer(Site);
        var sender = Sender(keepCookies: false);

        await sender.SendAsync(Get(server.BaseUri, "/login"), TestContext.Current.CancellationToken);
        await sender.SendAsync(Get(server.BaseUri, "/me"), TestContext.Current.CancellationToken);

        Assert.All(server.Seen, seen => Assert.Null(seen.Cookie));
    }

    [Fact]
    public async Task A_cookie_header_written_on_the_request_is_sent_as_written()
    {
        using var server = new LoopbackServer(Site);
        var sender = Sender(keepCookies: true);

        await sender.SendAsync(Get(server.BaseUri, "/login"), TestContext.Current.CancellationToken);
        await sender.SendAsync(Get(server.BaseUri, "/me", new KeyValue("Cookie", "manual=1")), TestContext.Current.CancellationToken);

        Assert.Equal("manual=1", server.Seen[1].Cookie);
    }

    [Fact]
    public async Task Resetting_cookies_forgets_them()
    {
        using var server = new LoopbackServer(Site);
        var sender = Sender(keepCookies: true);

        await sender.SendAsync(Get(server.BaseUri, "/login"), TestContext.Current.CancellationToken);
        sender.ResetCookies();
        await sender.SendAsync(Get(server.BaseUri, "/me"), TestContext.Current.CancellationToken);

        Assert.Null(server.Seen[1].Cookie);
    }

    [Fact]
    public async Task Keeping_cookies_across_a_settings_change_keeps_them()
    {
        using var server = new LoopbackServer(Site);
        var sender = Sender(keepCookies: true);

        await sender.SendAsync(Get(server.BaseUri, "/login"), TestContext.Current.CancellationToken);
        sender.Configure(TimeSpan.FromSeconds(10), followRedirects: true, verifyCertificates: true, RequestSender.DefaultMaxBodyBytes, keepCookies: true);
        await sender.SendAsync(Get(server.BaseUri, "/me"), TestContext.Current.CancellationToken);

        Assert.Equal("session=abc123", server.Seen[1].Cookie);
    }

    [Fact]
    public async Task Turning_cookies_off_drops_them()
    {
        using var server = new LoopbackServer(Site);
        var sender = Sender(keepCookies: true);

        await sender.SendAsync(Get(server.BaseUri, "/login"), TestContext.Current.CancellationToken);
        sender.Configure(TimeSpan.FromSeconds(10), followRedirects: true, verifyCertificates: true, RequestSender.DefaultMaxBodyBytes, keepCookies: false);
        sender.Configure(TimeSpan.FromSeconds(10), followRedirects: true, verifyCertificates: true, RequestSender.DefaultMaxBodyBytes, keepCookies: true);
        await sender.SendAsync(Get(server.BaseUri, "/me"), TestContext.Current.CancellationToken);

        Assert.Null(server.Seen[1].Cookie);
    }

    [Fact]
    public async Task Cookies_are_not_written_to_any_file()
    {
        var folder = Path.Combine(Path.GetTempPath(), "ricochet-cookies-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(folder);
        try
        {
            using var server = new LoopbackServer(Site);
            var sender = Sender(keepCookies: true);

            await sender.SendAsync(Get(server.BaseUri, "/login"), TestContext.Current.CancellationToken);
            await sender.SendAsync(Get(server.BaseUri, "/me"), TestContext.Current.CancellationToken);

            var files = Directory.GetFiles(folder, "*", SearchOption.AllDirectories);
            Assert.Empty(files);
            Assert.DoesNotContain("session=abc123", string.Concat(files.Select(File.ReadAllText)));
        }
        finally
        {
            Directory.Delete(folder, recursive: true);
        }
    }
}
