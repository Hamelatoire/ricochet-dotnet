using System.Net;
using Ricochet.Core;

namespace Ricochet.Core.Tests;

// A control character in a value sent on the wire is refused before anything is sent (ADR 0075).
public class ControlCharacterTests
{
    [Theory]
    [InlineData("\n")]
    [InlineData("\r")]
    [InlineData("\0")]
    [InlineData("\x1b")]
    public async Task A_control_character_in_the_URL_is_refused_and_nothing_is_sent(string control)
    {
        var handler = new CountingHandler();
        var sender = new RequestSender(new HttpClient(handler));

        var error = await Assert.ThrowsAsync<RequestFailedException>(() =>
            sender.SendAsync(new ApiRequest { Method = "GET", Url = $"http://x.test/a{control}b" }));

        Assert.Contains("URL", error.Message);
        Assert.Equal(0, handler.Sent);
    }

    [Fact]
    public async Task A_line_break_in_a_query_value_from_the_URL_is_refused()
    {
        var handler = new CountingHandler();
        var sender = new RequestSender(new HttpClient(handler));

        await Assert.ThrowsAsync<RequestFailedException>(() =>
            sender.SendAsync(new ApiRequest { Method = "GET", Url = "http://x.test/a?q=one%0Atwo\nX-Injected: yes" }));

        Assert.Equal(0, handler.Sent);
    }

    [Fact]
    public async Task A_line_break_in_a_header_value_is_refused_and_names_the_header()
    {
        var handler = new CountingHandler();
        var sender = new RequestSender(new HttpClient(handler));

        var error = await Assert.ThrowsAsync<RequestFailedException>(() =>
            sender.SendAsync(new ApiRequest
            {
                Method = "GET",
                Url = "http://x.test/",
                Headers = [new KeyValue("X-Trace", "abc\r\nX-Injected: yes")],
            }));

        Assert.Contains("X-Trace", error.Message);
        Assert.Equal(0, handler.Sent);
    }

    [Fact]
    public async Task A_control_character_in_a_header_name_is_refused()
    {
        var handler = new CountingHandler();
        var sender = new RequestSender(new HttpClient(handler));

        await Assert.ThrowsAsync<RequestFailedException>(() =>
            sender.SendAsync(new ApiRequest
            {
                Method = "GET",
                Url = "http://x.test/",
                Headers = [new KeyValue("X-Bad\nName", "value")],
            }));

        Assert.Equal(0, handler.Sent);
    }

    [Fact]
    public async Task A_line_break_in_a_Bearer_token_is_refused()
    {
        var handler = new CountingHandler();
        var sender = new RequestSender(new HttpClient(handler));

        var error = await Assert.ThrowsAsync<RequestFailedException>(() =>
            sender.SendAsync(new ApiRequest
            {
                Method = "GET",
                Url = "http://x.test/",
                Auth = new RequestAuth { Kind = AuthKinds.Bearer, Token = "abc\nX-Injected: yes" },
            }));

        Assert.Contains("Authorization", error.Message);
        Assert.Equal(0, handler.Sent);
    }

    [Fact]
    public async Task A_control_character_in_an_API_key_sent_in_the_query_is_refused()
    {
        var handler = new CountingHandler();
        var sender = new RequestSender(new HttpClient(handler));

        var error = await Assert.ThrowsAsync<RequestFailedException>(() =>
            sender.SendAsync(new ApiRequest
            {
                Method = "GET",
                Url = "http://x.test/",
                Auth = new RequestAuth { Kind = AuthKinds.ApiKey, KeyInQuery = true, KeyName = "key", KeyValue = "a\rb" },
            }));

        Assert.Contains("key", error.Message);
        Assert.Equal(0, handler.Sent);
    }

    [Fact]
    public async Task A_tab_in_a_header_value_is_still_allowed()
    {
        var handler = new CountingHandler();
        var sender = new RequestSender(new HttpClient(handler));

        var response = await sender.SendAsync(new ApiRequest
        {
            Method = "GET",
            Url = "http://x.test/",
            Headers = [new KeyValue("X-Tabbed", "a\tb")],
        });

        Assert.Equal(200, response.StatusCode);
        Assert.Equal(1, handler.Sent);
    }

    [Fact]
    public async Task A_line_break_in_a_JSON_body_is_still_allowed()
    {
        var handler = new CountingHandler();
        var sender = new RequestSender(new HttpClient(handler));

        var response = await sender.SendAsync(new ApiRequest
        {
            Method = "POST",
            Url = "http://x.test/",
            Body = "{\n  \"name\": \"one\"\n}",
        });

        Assert.Equal(200, response.StatusCode);
        Assert.Equal(1, handler.Sent);
    }

    private sealed class CountingHandler : HttpMessageHandler
    {
        public int Sent { get; private set; }

        protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken)
        {
            Sent++;
            return Task.FromResult(new HttpResponseMessage(HttpStatusCode.OK));
        }
    }
}
