using Ricochet.Core;

namespace Ricochet.Core.Tests;

/// <summary>
/// ADR 0012: a collection run can keep cookies between its own Requests. Cookie storage happens in the real
/// SocketsHttpHandler, so these tests use the loopback server from <see cref="SessionCookiesTests"/>, not a fake handler.
/// </summary>
public class RunCookiesTests
{
    private static SessionCookiesTests.Reply Site(SessionCookiesTests.Seen seen) => seen.Path switch
    {
        "/login" => new(200, ["Set-Cookie: session=abc123; Path=/"], "ok"),
        _ => new(200, [], "hello"),
    };

    private static CollectionRunner Runner() => new(new RequestSender(RequestSender.CreateDefaultClient()));

    private static Workspace Shop(params CollectionItem[] items) =>
        new() { Collections = [new Collection { Name = "Shop", Items = items }] };

    private static CollectionItem Item(string name, Uri baseUri, string path) =>
        new() { Id = name, Name = name, Request = new ApiRequest { Name = name, Method = "GET", Url = new Uri(baseUri, path).ToString() } };

    private static Task<RunReport> Run(CollectionRunner runner, Workspace workspace, bool keepCookies) =>
        runner.RunAsync(workspace, new RunOptions { KeepCookies = keepCookies }, cancellationToken: TestContext.Current.CancellationToken);

    [Fact]
    public void Keeping_cookies_is_off_by_default()
    {
        Assert.False(new RunOptions().KeepCookies);
    }

    [Fact]
    public async Task A_cookie_set_by_one_request_is_sent_on_the_next_request_of_the_same_run()
    {
        using var server = new SessionCookiesTests.LoopbackServer(Site);
        var workspace = Shop(Item("Login", server.BaseUri, "/login"), Item("Me", server.BaseUri, "/me"));

        var report = await Run(Runner(), workspace, keepCookies: true);

        Assert.True(report.Passed);
        Assert.Null(server.Seen[0].Cookie);
        Assert.Equal("session=abc123", server.Seen[1].Cookie);
    }

    [Fact]
    public async Task Cookies_are_cleared_when_a_run_ends()
    {
        using var server = new SessionCookiesTests.LoopbackServer(Site);
        var runner = Runner();
        var login = Shop(Item("Login", server.BaseUri, "/login"), Item("Me", server.BaseUri, "/me"));
        var me = Shop(Item("Me", server.BaseUri, "/me"));

        await Run(runner, login, keepCookies: true);
        await Run(runner, me, keepCookies: true);

        // The second run's only request is the third one the server saw, and it must not carry the first run's session.
        Assert.Equal("/me", server.Seen[2].Path);
        Assert.Null(server.Seen[2].Cookie);
    }

    [Fact]
    public async Task Without_keeping_cookies_a_run_sends_no_cookies_between_its_requests()
    {
        using var server = new SessionCookiesTests.LoopbackServer(Site);
        var workspace = Shop(Item("Login", server.BaseUri, "/login"), Item("Me", server.BaseUri, "/me"));

        await Run(Runner(), workspace, keepCookies: false);

        Assert.Equal(2, server.Seen.Count);
        Assert.Null(server.Seen[1].Cookie);
    }

    [Fact]
    public async Task A_run_that_keeps_cookies_does_not_see_or_clear_the_senders_own_cookies()
    {
        using var server = new SessionCookiesTests.LoopbackServer(Site);
        var sender = new RequestSender(RequestSender.CreateDefaultClient());
        sender.Configure(TimeSpan.FromSeconds(10), followRedirects: true, verifyCertificates: true, RequestSender.DefaultMaxBodyBytes, keepCookies: true);
        var token = TestContext.Current.CancellationToken;

        await sender.SendAsync(new ApiRequest { Name = "Login", Method = "GET", Url = new Uri(server.BaseUri, "/login").ToString() }, token);
        var run = Shop(Item("Me", server.BaseUri, "/me"));
        await new CollectionRunner(sender).RunAsync(run, new RunOptions { KeepCookies = true }, cancellationToken: token);
        await sender.SendAsync(new ApiRequest { Name = "Me", Method = "GET", Url = new Uri(server.BaseUri, "/me").ToString() }, token);

        Assert.Null(server.Seen[1].Cookie);
        Assert.Equal("session=abc123", server.Seen[2].Cookie);
    }
}
