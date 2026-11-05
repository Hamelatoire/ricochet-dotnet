using System.Net;
using System.Text;
using Ricochet.Core;

namespace Ricochet.Core.Tests;

/// <summary>Checks what actually leaves the app, not only what the resolver returns.</summary>
public class VariablesInRequestsTests
{
    private sealed class CapturingHandler : HttpMessageHandler
    {
        public HttpRequestMessage? Received { get; private set; }
        public string? Body { get; private set; }

        protected override async Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken)
        {
            Received = request;
            if (request.Content is not null)
                Body = await request.Content.ReadAsStringAsync(cancellationToken);
            return new HttpResponseMessage(HttpStatusCode.OK);
        }
    }

    private static async Task<CapturingHandler> SendAsync(ApiRequest request, params KeyValue[] variables)
    {
        var handler = new CapturingHandler();
        var resolved = VariableResolver.Apply(request, variables);
        Assert.Empty(VariableResolver.FindPlaceholders(resolved));
        await new RequestSender(new HttpClient(handler)).SendAsync(resolved, TestContext.Current.CancellationToken);
        return handler;
    }

    private static ApiRequest Request(RequestAuth? auth = null) => new() { Url = "http://x.test/", Auth = auth };

    [Fact]
    public async Task Body_is_sent_with_variables_replaced()
    {
        var sent = await SendAsync(
            Request() with { Method = "POST", Body = """{"user":"{{user}}","count":{{count}}}""" },
            new("user", "ada"), new("count", "3"));

        Assert.Equal("""{"user":"ada","count":3}""", sent.Body);
        Assert.Equal("application/json", sent.Received!.Content!.Headers.ContentType!.MediaType);
    }

    [Fact]
    public async Task Header_name_and_value_are_sent_with_variables_replaced()
    {
        var sent = await SendAsync(
            Request() with { Headers = [new("X-{{name}}", "before-{{value}}-after")] },
            new("name", "Tenant"), new("value", "acme"));

        Assert.Equal(["before-acme-after"], sent.Received!.Headers.GetValues("X-Tenant"));
    }

    [Fact]
    public async Task Content_header_is_sent_with_variables_replaced()
    {
        var sent = await SendAsync(
            Request() with { Method = "POST", Body = "<a/>", Headers = [new("Content-Type", "{{type}}")] },
            new KeyValue("type", "application/xml"));

        Assert.Equal("application/xml", sent.Received!.Content!.Headers.ContentType!.MediaType);
    }

    [Fact]
    public async Task Bearer_token_is_sent_with_variables_replaced()
    {
        var sent = await SendAsync(Request(new RequestAuth { Kind = AuthKinds.Bearer, Token = "{{token}}" }), new KeyValue("token", "abc"));

        Assert.Equal("Bearer abc", sent.Received!.Headers.Authorization!.ToString());
    }

    [Fact]
    public async Task Basic_username_and_password_are_sent_with_variables_replaced()
    {
        var sent = await SendAsync(
            Request(new RequestAuth { Kind = AuthKinds.Basic, Username = "{{user}}", Password = "{{pass}}" }),
            new("user", "ada"), new("pass", "s3cret"));

        var expected = Convert.ToBase64String(Encoding.UTF8.GetBytes("ada:s3cret"));
        Assert.Equal($"Basic {expected}", sent.Received!.Headers.Authorization!.ToString());
    }

    [Fact]
    public async Task Api_key_header_is_sent_with_variables_replaced()
    {
        var sent = await SendAsync(
            Request(new RequestAuth { Kind = AuthKinds.ApiKey, KeyName = "{{header}}", KeyValue = "{{key}}" }),
            new("header", "X-Api-Key"), new("key", "k-1"));

        Assert.Equal(["k-1"], sent.Received!.Headers.GetValues("X-Api-Key"));
    }

    [Fact]
    public async Task Api_key_in_the_query_string_is_sent_with_variables_replaced_and_escaped()
    {
        var sent = await SendAsync(
            Request(new RequestAuth { Kind = AuthKinds.ApiKey, KeyName = "{{param}}", KeyValue = "{{key}}", KeyInQuery = true }),
            new("param", "api_key"), new("key", "a b&c"));

        Assert.Equal("?api_key=a%20b%26c", sent.Received!.RequestUri!.Query);
    }

    [Fact]
    public async Task A_secret_variable_works_in_the_body_a_header_and_auth()
    {
        var sent = await SendAsync(
            new ApiRequest
            {
                Method = "POST",
                Url = "http://x.test/",
                Body = "{{secret}}",
                Headers = [new("X-Secret", "{{secret}}")],
                Auth = new RequestAuth { Kind = AuthKinds.Bearer, Token = "{{secret}}" },
            },
            new KeyValue("secret", "hidden", IsSecret: true));

        Assert.Equal("hidden", sent.Body);
        Assert.Equal(["hidden"], sent.Received!.Headers.GetValues("X-Secret"));
        Assert.Equal("Bearer hidden", sent.Received.Headers.Authorization!.ToString());
    }

    [Fact]
    public async Task A_nested_variable_works_in_auth()
    {
        var sent = await SendAsync(
            Request(new RequestAuth { Kind = AuthKinds.Bearer, Token = "{{token}}" }),
            new("token", "prefix-{{inner}}"), new("inner", "abc"));

        Assert.Equal("Bearer prefix-abc", sent.Received!.Headers.Authorization!.ToString());
    }

    // The app keeps the other auth types' fields when you switch, so that switching back loses nothing.
    [Theory]
    [InlineData(AuthKinds.Bearer, "token")]
    [InlineData(AuthKinds.Basic, "user,pass")]
    [InlineData(AuthKinds.ApiKey, "name,value")]
    public void Only_the_chosen_auth_type_can_report_a_missing_variable(string kind, string expected)
    {
        var request = new ApiRequest
        {
            Auth = new RequestAuth
            {
                Kind = kind,
                Token = "{{token}}",
                Username = "{{user}}",
                Password = "{{pass}}",
                KeyName = "{{name}}",
                KeyValue = "{{value}}",
            },
        };

        Assert.Equal(expected.Split(','), VariableResolver.FindPlaceholders(request));
    }

    [Fact]
    public void A_disabled_header_cannot_report_a_missing_variable()
    {
        var request = new ApiRequest { Headers = [new("X-Off", "{{missing}}", Enabled: false)] };

        Assert.Empty(VariableResolver.FindPlaceholders(request));
    }
}
