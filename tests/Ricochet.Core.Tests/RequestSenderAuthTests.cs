using System.Net;
using System.Net.Sockets;
using System.Security.Cryptography;
using System.Text;
using System.Text.RegularExpressions;
using Ricochet.Core;

namespace Ricochet.Core.Tests;

// Auth types as RequestSender sends them (ADR 0024), using a fake handler so nothing leaves the test.
public partial class RequestSenderAuthTests
{
    private const string Realm = "testrealm@host.com";
    private const string Nonce = "dcd98b7102dd2f0e8b11d0f600bfb0c093";
    private const string Opaque = "5ccc069c403ebaf9f0171e9517f40e41";

    [GeneratedRegex("(\\w+)=(?:\"((?:[^\"\\\\]|\\\\.)*)\"|([^,\\s]+))")]
    private static partial Regex AuthParam();

    // ---- Digest ----

    [Fact]
    public async Task Digest_retry_carries_an_answer_that_the_server_can_check()
    {
        var sent = new List<string?>();
        var handler = new CapturingHandler(request =>
        {
            var authorization = request.Headers.Authorization?.ToString();
            sent.Add(authorization);
            if (authorization is null)
                return Challenge(HttpStatusCode.Unauthorized, $"Digest realm=\"{Realm}\", qop=\"auth\", nonce=\"{Nonce}\", opaque=\"{Opaque}\"");

            return new HttpResponseMessage(HttpStatusCode.OK);
        });
        var sender = new RequestSender(new HttpClient(handler));

        var response = await sender.SendAsync(new ApiRequest
        {
            Method = "GET",
            Url = "http://x.test/dir/index.html?a=1",
            Auth = new RequestAuth { Kind = AuthKinds.Digest, Username = "Mufasa", Password = "Circle Of Life" },
        });

        Assert.Equal(200, response.StatusCode);
        Assert.Equal(2, sent.Count);
        Assert.Null(sent[0]);

        var retry = Params(sent[1]!);
        Assert.Equal("Mufasa", retry["username"]);
        Assert.Equal(Realm, retry["realm"]);
        Assert.Equal(Nonce, retry["nonce"]);
        Assert.Equal("/dir/index.html?a=1", retry["uri"]);
        Assert.Equal("MD5", retry["algorithm"]);
        Assert.Equal("auth", retry["qop"]);
        Assert.Equal("00000001", retry["nc"]);
        Assert.Equal(Opaque, retry["opaque"]);
        Assert.Matches("^[0-9a-f]{32}$", retry["cnonce"]);

        // Worked out again from RFC 7616 (MD5, qop auth) with the client nonce the header carries, not by calling AuthSigning.
        var ha1 = Md5($"Mufasa:{Realm}:Circle Of Life");
        var ha2 = Md5("GET:/dir/index.html?a=1");
        Assert.Equal(Md5($"{ha1}:{Nonce}:00000001:{retry["cnonce"]}:auth:{ha2}"), retry["response"]);
    }

    [Fact]
    public async Task Digest_challenge_with_an_unknown_algorithm_stops_after_the_401()
    {
        var count = 0;
        var handler = new CapturingHandler(_ =>
        {
            count++;
            return Challenge(HttpStatusCode.Unauthorized, $"Digest realm=\"{Realm}\", nonce=\"{Nonce}\", algorithm=SHA-512-256");
        });
        var sender = new RequestSender(new HttpClient(handler));

        var ex = await Assert.ThrowsAsync<RequestFailedException>(() => sender.SendAsync(new ApiRequest
        {
            Url = "http://x.test/",
            Auth = new RequestAuth { Kind = AuthKinds.Digest, Username = "u", Password = "p" },
        }));

        Assert.Equal(1, count);
        Assert.Contains("SHA-512-256", ex.Message);
    }

    [Fact]
    public async Task Digest_challenge_with_a_qop_it_cannot_answer_stops_after_the_401()
    {
        var count = 0;
        var handler = new CapturingHandler(_ =>
        {
            count++;
            return Challenge(HttpStatusCode.Unauthorized, $"Digest realm=\"{Realm}\", nonce=\"{Nonce}\", qop=\"auth-int\"");
        });
        var sender = new RequestSender(new HttpClient(handler));

        var ex = await Assert.ThrowsAsync<RequestFailedException>(() => sender.SendAsync(new ApiRequest
        {
            Url = "http://x.test/",
            Auth = new RequestAuth { Kind = AuthKinds.Digest, Username = "u", Password = "p" },
        }));

        Assert.Equal(1, count);
        Assert.Contains("auth-int", ex.Message);
    }

    // ---- OAuth 1.0 ----

    [Fact]
    public void OAuth1_query_parameters_sign_the_same_as_the_same_fields_in_a_form_body()
    {
        // RFC 5849 section 3.4.1.3: query and form fields both enter the signature base string, so moving them changes nothing.
        var auth = OAuthAuth();

        var inQuery = AuthSigning.OAuth1Authorization(auth, "POST", new Uri("https://x.test/a?status=Hello%20World&include=true"), [], "n1", 100);
        var inBody = AuthSigning.OAuth1Authorization(auth, "POST", new Uri("https://x.test/a"),
            [new("status", "Hello World"), new("include", "true")], "n1", 100);
        var none = AuthSigning.OAuth1Authorization(auth, "POST", new Uri("https://x.test/a"), [], "n1", 100);

        Assert.Equal(inQuery, inBody);
        Assert.NotEqual(none, inQuery);
    }

    [Fact]
    public void OAuth1_query_plus_signs_are_spaces_in_the_signature()
    {
        var auth = OAuthAuth();

        var plus = AuthSigning.OAuth1Authorization(auth, "GET", new Uri("https://x.test/a?q=a+b"), [], "n", 1);
        var percent = AuthSigning.OAuth1Authorization(auth, "GET", new Uri("https://x.test/a?q=a%20b"), [], "n", 1);

        Assert.Equal(percent, plus);
    }

    [Fact]
    public async Task OAuth1_through_the_sender_signs_a_form_body_but_not_a_json_body()
    {
        var (formHeader, formParams) = await SendOAuth1("application/x-www-form-urlencoded", "status=Hello+World&include=true");
        var (jsonHeader, jsonParams) = await SendOAuth1("application/json", "{\"status\":\"Hello World\"}");

        // The sender picks the nonce and timestamp itself, so read them back from the header and sign again.
        Assert.Equal(
            AuthSigning.OAuth1Authorization(OAuthAuth(), "POST", new Uri("https://x.test/a?include=1"),
                [new("status", "Hello World"), new("include", "true")], formParams["oauth_nonce"], long.Parse(formParams["oauth_timestamp"])),
            formHeader);
        Assert.Equal(
            AuthSigning.OAuth1Authorization(OAuthAuth(), "POST", new Uri("https://x.test/a?include=1"), [], jsonParams["oauth_nonce"], long.Parse(jsonParams["oauth_timestamp"])),
            jsonHeader);
    }

    // ---- AWS Signature Version 4 ----

    [Fact]
    public async Task AwsSigV4_without_a_session_token_signs_host_and_x_amz_date_only()
    {
        var sent = await SendSigV4(new AuthDetails
        {
            AwsAccessKeyId = "AKIDEXAMPLE",
            AwsSecretAccessKey = "secret",
            AwsRegion = "us-east-1",
            AwsService = "service",
        });

        Assert.Null(sent.Headers.GetValueOrDefault("X-Amz-Security-Token"));
        Assert.Matches("^\\d{8}T\\d{6}Z$", sent.Headers["X-Amz-Date"]);
        var authorization = sent.Headers["Authorization"];
        Assert.StartsWith("AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/", authorization);
        Assert.Contains(", SignedHeaders=host;x-amz-date, Signature=", authorization);
    }

    [Fact]
    public async Task AwsSigV4_with_a_session_token_sends_the_token_header_and_signs_it()
    {
        var sent = await SendSigV4(new AuthDetails
        {
            AwsAccessKeyId = "AKIDEXAMPLE",
            AwsSecretAccessKey = "secret",
            AwsSessionToken = "session-token-value",
            AwsRegion = "us-east-1",
            AwsService = "execute-api",
        });

        Assert.Equal("session-token-value", sent.Headers["X-Amz-Security-Token"]);
        Assert.Contains("SignedHeaders=host;x-amz-date;x-amz-security-token, ", sent.Headers["Authorization"]);
    }

    // ---- Bearer ----

    [Fact]
    public async Task Bearer_token_with_a_space_is_sent_as_it_was_written()
    {
        // ADR 0024 does not cover Bearer validation, so this pins what the sender does today.
        var sent = await SendAndCapture(new RequestAuth { Kind = AuthKinds.Bearer, Token = "abc def" });

        Assert.Equal("Bearer abc def", sent.Headers["Authorization"]);
    }

    // A Bearer token with a line break would otherwise be written to the wire as a second header.
    [Fact]
    public async Task Bearer_token_with_a_newline_is_rejected_and_never_sent_as_a_second_header()
    {
        using var listener = new TcpListener(IPAddress.Loopback, 0);
        listener.Start();
        var port = ((IPEndPoint)listener.LocalEndpoint).Port;
        var accepted = listener.AcceptTcpClientAsync();
        var sender = new RequestSender(new HttpClient());

        await Assert.ThrowsAnyAsync<RequestFailedException>(() => sender.SendAsync(new ApiRequest
        {
            Url = $"http://127.0.0.1:{port}/",
            Auth = new RequestAuth { Kind = AuthKinds.Bearer, Token = "abc\r\nX-Injected: yes" },
        }));

        if (accepted.IsCompletedSuccessfully)
        {
            using var client = accepted.Result;
            client.ReceiveTimeout = 2000;
            var buffer = new byte[4096];
            var read = client.GetStream().Read(buffer, 0, buffer.Length);
            Assert.DoesNotContain("X-Injected", Encoding.ASCII.GetString(buffer, 0, read));
        }
    }

    // ---- helpers ----

    private static RequestAuth OAuthAuth() => new()
    {
        Kind = AuthKinds.OAuth1,
        Token = "token",
        Details = new AuthDetails { ConsumerKey = "consumer", ConsumerSecret = "consumer-secret", TokenSecret = "token-secret" },
    };

    private static async Task<(string Header, Dictionary<string, string> Params)> SendOAuth1(string contentType, string body)
    {
        string? authorization = null;
        var handler = new CapturingHandler(request =>
        {
            authorization = request.Headers.Authorization?.ToString();
            return new HttpResponseMessage(HttpStatusCode.OK);
        });
        var sender = new RequestSender(new HttpClient(handler));

        await sender.SendAsync(new ApiRequest
        {
            Method = "POST",
            Url = "https://x.test/a?include=1",
            Headers = [new("Content-Type", contentType)],
            Body = body,
            Auth = OAuthAuth(),
        });

        return (authorization!, Params(authorization!));
    }

    private static async Task<SentRequest> SendSigV4(AuthDetails details, string url = "https://example.amazonaws.com/")
    {
        return await SendAndCapture(new RequestAuth { Kind = AuthKinds.AwsSigV4, Details = details }, url);
    }

    private static async Task<SentRequest> SendAndCapture(RequestAuth auth, string url = "https://x.test/")
    {
        var headers = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
        var handler = new CapturingHandler(request =>
        {
            foreach (var header in request.Headers)
                headers[header.Key] = string.Join(",", header.Value);
            return new HttpResponseMessage(HttpStatusCode.OK);
        });
        var sender = new RequestSender(new HttpClient(handler));

        await sender.SendAsync(new ApiRequest { Url = url, Auth = auth });
        return new SentRequest(headers);
    }

    private static HttpResponseMessage Challenge(HttpStatusCode status, string challenge)
    {
        var response = new HttpResponseMessage(status);
        response.Headers.TryAddWithoutValidation("WWW-Authenticate", challenge);
        return response;
    }

    private static Dictionary<string, string> Params(string header)
    {
        var result = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
        foreach (Match m in AuthParam().Matches(header))
            result[m.Groups[1].Value] = m.Groups[2].Success ? m.Groups[2].Value : m.Groups[3].Value;
        return result;
    }

    private static string Md5(string text) => Convert.ToHexString(MD5.HashData(Encoding.UTF8.GetBytes(text))).ToLowerInvariant();

    private sealed record SentRequest(Dictionary<string, string> Headers);

    private sealed class CapturingHandler(Func<HttpRequestMessage, HttpResponseMessage> respond) : HttpMessageHandler
    {
        protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken) =>
            Task.FromResult(respond(request));
    }
}
