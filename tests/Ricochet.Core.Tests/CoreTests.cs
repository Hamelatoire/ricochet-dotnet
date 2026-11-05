using System.Net;
using System.Text;
using Ricochet.Core;

namespace Ricochet.Core.Tests;

public class RequestSenderTests
{
    private sealed class FakeHandler(Func<HttpRequestMessage, CancellationToken, Task<HttpResponseMessage>> respond) : HttpMessageHandler
    {
        public HttpRequestMessage? Received { get; private set; }
        public string? ReceivedBody { get; private set; }

        protected override async Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken)
        {
            Received = request;
            if (request.Content is not null)
                ReceivedBody = await request.Content.ReadAsStringAsync(cancellationToken);
            return await respond(request, cancellationToken);
        }
    }

    private static FakeHandler Ok(string body = "ok") =>
        new((_, _) => Task.FromResult(new HttpResponseMessage(HttpStatusCode.OK) { Content = new StringContent(body) }));

    private static RequestSender Sender(HttpMessageHandler handler, TimeSpan? timeout = null, long maxBody = RequestSender.DefaultMaxBodyBytes) =>
        new(new HttpClient(handler) { Timeout = Timeout.InfiniteTimeSpan }, timeout, maxBody);

    [Theory]
    [InlineData("localhost:5000/ping", "http://localhost:5000/ping")]
    [InlineData("example.com?next=http://other.test", "http://example.com/?next=http://other.test")]
    [InlineData("  https://example.com/a  ", "https://example.com/a")]
    public async Task Normalizes_urls(string input, string expected)
    {
        var handler = Ok();
        await Sender(handler).SendAsync(new ApiRequest { Url = input }, TestContext.Current.CancellationToken);
        Assert.Equal(expected, handler.Received!.RequestUri!.ToString());
    }

    [Theory]
    [InlineData("")]
    [InlineData("ftp://example.com")]
    public async Task Rejects_unusable_urls(string url)
    {
        await Assert.ThrowsAsync<RequestFailedException>(() =>
            Sender(Ok()).SendAsync(new ApiRequest { Url = url }, TestContext.Current.CancellationToken));
    }

    [Fact]
    public async Task Sends_method_headers_and_body()
    {
        var handler = Ok();
        var request = new ApiRequest
        {
            Method = "POST",
            Url = "http://x.test",
            Body = "hello",
            Headers = [new("X-On", "1"), new("X-Off", "1", Enabled: false), new("Content-Type", "text/csv")],
        };

        await Sender(handler).SendAsync(request, TestContext.Current.CancellationToken);

        Assert.Equal("POST", handler.Received!.Method.Method);
        Assert.Equal("hello", handler.ReceivedBody);
        Assert.True(handler.Received.Headers.Contains("X-On"));
        Assert.False(handler.Received.Headers.Contains("X-Off"));
        Assert.Equal("text/csv", handler.Received.Content!.Headers.ContentType!.MediaType);
    }

    [Theory]
    [InlineData("{\"a\":1}", "application/json")]
    [InlineData("  [1]", "application/json")]
    [InlineData("plain", "text/plain")]
    public async Task Guesses_content_type_when_not_set(string body, string expected)
    {
        var handler = Ok();
        await Sender(handler).SendAsync(new ApiRequest { Method = "POST", Url = "http://x.test", Body = body }, TestContext.Current.CancellationToken);
        Assert.Equal(expected, handler.Received!.Content!.Headers.ContentType!.MediaType);
    }

    [Fact]
    public async Task Content_header_without_a_body_is_ignored()
    {
        var handler = Ok();
        var request = new ApiRequest { Url = "http://x.test", Headers = [new("Content-Type", "application/json")] };
        await Sender(handler).SendAsync(request, TestContext.Current.CancellationToken);
        Assert.Null(handler.Received!.Content);
    }

    [Fact]
    public async Task Invalid_header_name_is_reported()
    {
        var request = new ApiRequest { Url = "http://x.test", Headers = [new("bad header", "1")] };
        var ex = await Assert.ThrowsAsync<RequestFailedException>(() =>
            Sender(Ok()).SendAsync(request, TestContext.Current.CancellationToken));
        Assert.Contains("bad header", ex.Message);
    }

    [Fact]
    public async Task Network_failure_becomes_RequestFailedException()
    {
        var handler = new FakeHandler((_, _) => throw new HttpRequestException("No such host"));
        var ex = await Assert.ThrowsAsync<RequestFailedException>(() =>
            Sender(handler).SendAsync(new ApiRequest { Url = "http://x.test" }, TestContext.Current.CancellationToken));
        Assert.Equal("No such host", ex.Message);
    }

    private static FakeHandler Hanging() => new(async (_, ct) =>
    {
        await Task.Delay(Timeout.Infinite, ct);
        throw new InvalidOperationException("unreachable");
    });

    [Fact]
    public async Task User_cancel_is_distinct_from_timeout()
    {
        using var cts = CancellationTokenSource.CreateLinkedTokenSource(TestContext.Current.CancellationToken);
        var send = Sender(Hanging()).SendAsync(new ApiRequest { Url = "http://x.test" }, cts.Token);
        cts.Cancel();
        await Assert.ThrowsAnyAsync<OperationCanceledException>(() => send);
    }

    [Fact]
    public async Task Timeout_is_reported_as_failure()
    {
        var ex = await Assert.ThrowsAsync<RequestFailedException>(() =>
            Sender(Hanging(), TimeSpan.FromMilliseconds(20)).SendAsync(new ApiRequest { Url = "http://x.test" }, TestContext.Current.CancellationToken));
        Assert.Contains("Timed out", ex.Message);
    }

    [Fact]
    public async Task Oversized_body_is_truncated_at_the_limit()
    {
        var response = await Sender(Ok("0123456789"), maxBody: 4).SendAsync(new ApiRequest { Url = "http://x.test" }, TestContext.Current.CancellationToken);
        Assert.True(response.Truncated);
        Assert.Equal("0123"u8.ToArray(), response.Body);
    }

    [Fact]
    public async Task Body_exactly_at_the_limit_is_not_truncated()
    {
        var response = await Sender(Ok("0123"), maxBody: 4).SendAsync(new ApiRequest { Url = "http://x.test" }, TestContext.Current.CancellationToken);
        Assert.False(response.Truncated);
    }

    [Fact]
    public async Task Repeated_response_headers_are_all_kept()
    {
        var handler = new FakeHandler((_, _) =>
        {
            var message = new HttpResponseMessage(HttpStatusCode.OK);
            message.Headers.Add("Set-Cookie", ["a=1", "b=2"]);
            return Task.FromResult(message);
        });

        var response = await Sender(handler).SendAsync(new ApiRequest { Url = "http://x.test" }, TestContext.Current.CancellationToken);

        Assert.Equal(["a=1", "b=2"], response.Headers.Where(h => h.Key == "Set-Cookie").Select(h => h.Value));
    }
}

public class VariableResolverTests
{
    [Fact]
    public void Replaces_in_url_headers_and_body_and_leaves_unknown_names()
    {
        var request = new ApiRequest
        {
            Url = "{{ base }}/{{missing}}",
            Body = "{{token}}",
            Headers = [new("Authorization", "Bearer {{token}}")],
        };

        var resolved = VariableResolver.Apply(request, [new("base", "http://x.test"), new("token", "abc"), new("off", "1", Enabled: false)]);

        Assert.Equal("http://x.test/{{missing}}", resolved.Url);
        Assert.Equal("abc", resolved.Body);
        Assert.Equal("Bearer abc", resolved.Headers[0].Value);
    }

    [Fact]
    public void Disabled_variables_are_not_applied()
    {
        var resolved = VariableResolver.Apply(new ApiRequest { Url = "{{a}}" }, [new("a", "1", Enabled: false)]);
        Assert.Equal("{{a}}", resolved.Url);
    }
}

public class ResponseFormatterTests
{
    private static ApiResponse Response(byte[] body, string? mediaType = null, string? charSet = null, bool truncated = false) =>
        new(200, "OK", [], body, mediaType, charSet, TimeSpan.Zero, truncated);

    private static ApiResponse Response(string body, string? mediaType = null) => Response(Encoding.UTF8.GetBytes(body), mediaType);

    [Fact]
    public void Pretty_prints_json_without_altering_numbers_or_unicode()
    {
        var result = ResponseFormatter.Format(Response("{\"n\":12345678901234567890.10,\"s\":\"é\"}", "application/json"));
        Assert.True(result.IsJson);
        Assert.Contains("12345678901234567890.10", result.Text);
        Assert.Contains("é", result.Text);
        Assert.Contains("\n", result.Text);
    }

    [Fact]
    public void Invalid_json_is_shown_as_is()
    {
        var result = ResponseFormatter.Format(Response("{not json", "application/json"));
        Assert.False(result.IsJson);
        Assert.Equal("{not json", result.Text);
    }

    [Fact]
    public void Bom_does_not_break_json_detection()
    {
        var result = ResponseFormatter.Format(Response([0xEF, 0xBB, 0xBF, (byte)'{', (byte)'}'], "application/json"));
        Assert.True(result.IsJson);
    }

    [Fact]
    public void Truncated_json_is_not_parsed()
    {
        var result = ResponseFormatter.Format(Response("{}"u8.ToArray(), "application/json", truncated: true));
        Assert.False(result.IsJson);
    }

    [Fact]
    public void Nul_bytes_mean_binary_unless_a_charset_says_otherwise()
    {
        Assert.True(ResponseFormatter.Format(Response([1, 0, 2])).IsBinary);
        var utf16 = ResponseFormatter.Format(Response(Encoding.Unicode.GetBytes("hi"), "text/plain", "utf-16"));
        Assert.False(utf16.IsBinary);
        Assert.Equal("hi", utf16.Text);
    }

    [Fact]
    public void Unknown_charset_falls_back_to_utf8()
    {
        Assert.Equal("hi", ResponseFormatter.Format(Response("hi"u8.ToArray(), "text/plain", "not-a-charset")).Text);
    }
}
