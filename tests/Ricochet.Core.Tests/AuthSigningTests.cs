using System.Net;
using System.Text.Json;
using Ricochet.Core;

namespace Ricochet.Core.Tests;

public class AuthSigningTests
{
    // The worked example of RFC 2617, section 3.5 (MD5, qop auth).
    private const string Rfc2617Challenge =
        "realm=\"testrealm@host.com\", qop=\"auth,auth-int\", nonce=\"dcd98b7102dd2f0e8b11d0f600bfb0c093\", opaque=\"5ccc069c403ebaf9f0171e9517f40e41\"";

    [Fact]
    public void Digest_answer_matches_the_rfc_2617_example()
    {
        var auth = new RequestAuth { Kind = AuthKinds.Digest, Username = "Mufasa", Password = "Circle Of Life" };

        var header = AuthSigning.DigestAuthorization(auth, "GET", "/dir/index.html", Rfc2617Challenge, "0a4f113b");

        Assert.Contains("response=\"6629fae49393a05397450978507c4ef1\"", header);
        Assert.Contains("qop=auth, nc=00000001, cnonce=\"0a4f113b\"", header);
        Assert.Contains("opaque=\"5ccc069c403ebaf9f0171e9517f40e41\"", header);
    }

    [Fact]
    public void Digest_challenge_parsing_keeps_commas_inside_quotes()
    {
        var parsed = AuthSigning.ParseDigestChallenge("realm=\"a,b\", qop=\"auth,auth-int\", nonce=c, stale");

        Assert.Equal("a,b", parsed["realm"]);
        Assert.Equal("auth,auth-int", parsed["qop"]);
        Assert.Equal("c", parsed["nonce"]);
        Assert.False(parsed.ContainsKey("stale"));
    }

    [Theory]
    [InlineData("realm=\"r\", nonce=\"n\", algorithm=SHA-512-256", "algorithm")]
    [InlineData("realm=\"r\", nonce=\"n\", qop=\"auth-int\"", "qop")]
    public void Digest_refuses_challenges_it_cannot_answer(string challenge, string what)
    {
        var auth = new RequestAuth { Kind = AuthKinds.Digest, Username = "u", Password = "p" };

        var ex = Assert.Throws<RequestFailedException>(() => AuthSigning.DigestAuthorization(auth, "GET", "/", challenge, "cn"));
        Assert.Contains(what, ex.Message);
    }

    [Fact]
    public void OAuth1_signature_matches_the_twitter_example()
    {
        // The worked example in Twitter's OAuth documentation, with its fixed nonce and timestamp.
        var auth = new RequestAuth
        {
            Kind = AuthKinds.OAuth1,
            Token = "370773112-GmHxMAgYyLbNEtIKZeRNFsMKPR9EyMZeS9weJAEb",
            Details = new AuthDetails
            {
                ConsumerKey = "xvz1evFS4wEEPTGEFPHBog",
                ConsumerSecret = "kAcSOqF21Fu85e7zjz7ZN2U4ZRhfV3WpwPAoE3Z7kBw",
                TokenSecret = "LswwdoUaIvS8ltyTt5jkRh4J50vUPVVHtR2YPi5kE",
            },
        };
        var body = new[] { new KeyValuePair<string, string>("status", "Hello Ladies + Gentlemen, a signed OAuth request!") };

        var header = AuthSigning.OAuth1Authorization(
            auth,
            "POST",
            new Uri("https://api.twitter.com/1/statuses/update.json?include_entities=true"),
            body,
            "kYjzVBB8Y0ZFabxSWbWovY3uYSQ2pTgmZeNu2VS4cg",
            1318622958);

        Assert.StartsWith("OAuth ", header);
        Assert.Contains("oauth_signature=\"tnnArxj06cWHq44gCs1OSKk%2FjLY%3D\"", header);
        Assert.Contains("oauth_consumer_key=\"xvz1evFS4wEEPTGEFPHBog\"", header);
    }

    [Fact]
    public void OAuth1_header_never_contains_the_secrets()
    {
        var auth = new RequestAuth
        {
            Kind = AuthKinds.OAuth1,
            Token = "tok",
            Details = new AuthDetails { ConsumerKey = "ck", ConsumerSecret = "consumer-secret-value", TokenSecret = "token-secret-value" },
        };

        var header = AuthSigning.OAuth1Authorization(auth, "GET", new Uri("https://x.test/a"), [], "n", 1);

        Assert.DoesNotContain("consumer-secret-value", header);
        Assert.DoesNotContain("token-secret-value", header);
    }

    [Fact]
    public void OAuth1_needs_a_consumer_key()
    {
        Assert.Throws<RequestFailedException>(() =>
            AuthSigning.OAuth1Authorization(new RequestAuth { Kind = AuthKinds.OAuth1 }, "GET", new Uri("https://x.test/"), [], "n", 1));
    }

    [Fact]
    public void AwsSigV4_signature_matches_the_get_vanilla_example()
    {
        // The get-vanilla test case of the AWS Signature Version 4 test suite.
        var auth = new RequestAuth
        {
            Kind = AuthKinds.AwsSigV4,
            Details = new AuthDetails
            {
                AwsAccessKeyId = "AKIDEXAMPLE",
                AwsSecretAccessKey = "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
                AwsRegion = "us-east-1",
                AwsService = "service",
            },
        };

        var headers = AuthSigning.AwsSigV4Headers(auth, "GET", new Uri("https://example.amazonaws.com/"), [], new DateTimeOffset(2015, 8, 30, 12, 36, 0, TimeSpan.Zero));

        var authorization = headers.Single(h => h.Key == "Authorization").Value;
        Assert.Equal("20150830T123600Z", headers.Single(h => h.Key == "X-Amz-Date").Value);
        Assert.Equal(
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/service/aws4_request, SignedHeaders=host;x-amz-date, Signature=5fa00fa31553b73ebf1942676e86291e8372ff2a2260956d9b8aae1d763fbf31",
            authorization);
    }

    [Fact]
    public void AwsSigV4_adds_the_session_token_header_when_there_is_one()
    {
        var auth = new RequestAuth
        {
            Kind = AuthKinds.AwsSigV4,
            Details = new AuthDetails
            {
                AwsAccessKeyId = "AKID",
                AwsSecretAccessKey = "secret",
                AwsSessionToken = "session",
                AwsRegion = "eu-west-1",
                AwsService = "execute-api",
            },
        };

        var headers = AuthSigning.AwsSigV4Headers(auth, "POST", new Uri("https://x.test/items?b=2&a=1"), "{}"u8.ToArray(), DateTimeOffset.UtcNow);

        Assert.Equal("session", headers.Single(h => h.Key == "X-Amz-Security-Token").Value);
        Assert.Contains("SignedHeaders=host;x-amz-date;x-amz-security-token", headers.Single(h => h.Key == "Authorization").Value);
    }

    [Theory]
    [InlineData("", "execute-api")]
    [InlineData("us-east-1", "")]
    public void AwsSigV4_needs_region_and_service(string region, string service)
    {
        var auth = new RequestAuth
        {
            Kind = AuthKinds.AwsSigV4,
            Details = new AuthDetails { AwsAccessKeyId = "a", AwsSecretAccessKey = "b", AwsRegion = region, AwsService = service },
        };

        Assert.Throws<RequestFailedException>(() => AuthSigning.AwsSigV4Headers(auth, "GET", new Uri("https://x.test/"), [], DateTimeOffset.UtcNow));
    }

    [Fact]
    public async Task Digest_answers_a_challenge_with_one_retry_and_the_same_body()
    {
        var sent = new List<(string? Authorization, string Body)>();
        var handler = new Handler(request =>
        {
            var body = request.Content!.ReadAsStringAsync().GetAwaiter().GetResult();
            var authorization = request.Headers.TryGetValues("Authorization", out var values) ? string.Join(",", values) : null;
            sent.Add((authorization, body));

            if (authorization is null)
            {
                var challenge = new HttpResponseMessage(HttpStatusCode.Unauthorized);
                challenge.Headers.TryAddWithoutValidation("WWW-Authenticate", "Digest " + Rfc2617Challenge);
                return challenge;
            }

            return new HttpResponseMessage(HttpStatusCode.OK) { Content = new StringContent("done") };
        });
        var sender = new RequestSender(new HttpClient(handler));

        var response = await sender.SendAsync(new ApiRequest
        {
            Method = "POST",
            Url = "http://x.test/dir/index.html?a=1",
            Body = "{\"q\":1}",
            Auth = new RequestAuth { Kind = AuthKinds.Digest, Username = "Mufasa", Password = "Circle Of Life" },
        });

        Assert.Equal(200, response.StatusCode);
        Assert.Equal(2, sent.Count);
        Assert.Null(sent[0].Authorization);
        Assert.StartsWith("Digest username=\"Mufasa\"", sent[1].Authorization);
        Assert.Contains("uri=\"/dir/index.html?a=1\"", sent[1].Authorization);
        Assert.Equal("{\"q\":1}", sent[1].Body);
    }

    [Fact]
    public async Task Digest_without_a_challenge_returns_the_401_as_it_is()
    {
        var count = 0;
        var handler = new Handler(_ =>
        {
            count++;
            return new HttpResponseMessage(HttpStatusCode.Unauthorized);
        });
        var sender = new RequestSender(new HttpClient(handler));

        var response = await sender.SendAsync(new ApiRequest
        {
            Url = "http://x.test/",
            Auth = new RequestAuth { Kind = AuthKinds.Digest, Username = "u", Password = "p" },
        });

        Assert.Equal(401, response.StatusCode);
        Assert.Equal(1, count);
    }

    [Fact]
    public async Task AwsSigV4_sends_its_authorization_through_request_sender()
    {
        string? authorization = null;
        var handler = new Handler(request =>
        {
            authorization = request.Headers.TryGetValues("Authorization", out var values) ? string.Join(",", values) : null;
            return new HttpResponseMessage(HttpStatusCode.OK);
        });
        var sender = new RequestSender(new HttpClient(handler));

        await sender.SendAsync(new ApiRequest
        {
            Url = "https://example.amazonaws.com/",
            Auth = new RequestAuth
            {
                Kind = AuthKinds.AwsSigV4,
                Details = new AuthDetails
                {
                    AwsAccessKeyId = "AKIDEXAMPLE",
                    AwsSecretAccessKey = "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
                    AwsRegion = "us-east-1",
                    AwsService = "service",
                },
            },
        });

        Assert.StartsWith("AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/", authorization);
    }

    [Fact]
    public void A_request_with_no_details_serialises_without_them()
    {
        // The app's JSON context writes nulls as absent (WorkspaceJsonContext), so the same rule is used here.
        var options = new JsonSerializerOptions { DefaultIgnoreCondition = System.Text.Json.Serialization.JsonIgnoreCondition.WhenWritingNull };
        var json = JsonSerializer.Serialize(new RequestAuth { Kind = AuthKinds.Bearer, Token = "{{token}}" }, options);

        Assert.DoesNotContain("Details", json);
        Assert.DoesNotContain("Aws", json);
        Assert.DoesNotContain("Consumer", json);
    }

    private sealed class Handler(Func<HttpRequestMessage, HttpResponseMessage> respond) : HttpMessageHandler
    {
        protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken) =>
            Task.FromResult(respond(request));
    }
}

public class NewAuthSecretTests
{
    private const string Literal = "literal-secret-value";

    private static RequestAuth SecretAuth() => new()
    {
        Kind = AuthKinds.OAuth1,
        Token = "token-value",
        Details = new AuthDetails
        {
            ConsumerKey = "public-key",
            ConsumerSecret = Literal,
            TokenSecret = "token-secret-literal",
            AwsAccessKeyId = "AKID",
            AwsSecretAccessKey = "{{awsSecret}}",
            AwsSessionToken = "session-literal",
        },
    };

    private static Workspace WorkspaceWith(RequestAuth auth) => new()
    {
        Collections =
        [
            new Collection
            {
                Id = "c1",
                Items = [new CollectionItem { Name = "R", Request = new ApiRequest { Id = "r1", Name = "R", Url = "https://x.test", Auth = auth } }],
            },
        ],
    };

    [Fact]
    public void Secret_fields_leave_the_workspace_and_come_back_on_merge()
    {
        var (clean, secrets) = WorkspaceSecrets.Split(WorkspaceWith(SecretAuth()));

        var cleanDetails = clean.Collections[0].Items[0].Request!.Auth!.Details!;
        Assert.Equal("", cleanDetails.ConsumerSecret);
        Assert.Equal("", cleanDetails.TokenSecret);
        Assert.Equal("", cleanDetails.AwsSessionToken);
        Assert.Equal("public-key", cleanDetails.ConsumerKey);
        Assert.Equal("AKID", cleanDetails.AwsAccessKeyId);
        Assert.Equal("{{awsSecret}}", cleanDetails.AwsSecretAccessKey);

        Assert.Equal(Literal, secrets["auth/r1/consumerSecret"]);
        Assert.Equal("token-secret-literal", secrets["auth/r1/tokenSecret"]);
        Assert.Equal("session-literal", secrets["auth/r1/awsSessionToken"]);
        Assert.DoesNotContain(secrets, pair => pair.Value == "public-key");

        var merged = WorkspaceSecrets.Merge(clean, secrets).Collections[0].Items[0].Request!.Auth!.Details!;
        Assert.Equal(Literal, merged.ConsumerSecret);
        Assert.Equal("token-secret-literal", merged.TokenSecret);
        Assert.Equal("session-literal", merged.AwsSessionToken);
    }

    [Fact]
    public void Secret_fields_never_reach_the_saved_file()
    {
        var (clean, _) = WorkspaceSecrets.Split(WorkspaceWith(SecretAuth()));

        var json = JsonSerializer.Serialize(clean.Collections[0].Items[0].Request!.Auth);

        Assert.DoesNotContain(Literal, json);
        Assert.DoesNotContain("session-literal", json);
        Assert.DoesNotContain("token-secret-literal", json);
    }

    [Fact]
    public void History_hides_literal_values_in_new_fields()
    {
        var entry = new HistoryEntry(new ApiRequest { Url = "https://x.test", Auth = SecretAuth() }, 200, 1, DateTimeOffset.UtcNow);

        var sanitized = WorkspaceSecrets.SanitizeHistory(entry).Request.Auth!.Details!;

        Assert.Equal("", sanitized.ConsumerSecret);
        Assert.Equal("", sanitized.TokenSecret);
        Assert.Equal("", sanitized.AwsSessionToken);
        Assert.Equal("{{awsSecret}}", sanitized.AwsSecretAccessKey);
    }

    [Fact]
    public void Placeholders_in_new_auth_fields_are_found_when_the_request_is_sent()
    {
        var request = new ApiRequest
        {
            Url = "https://x.test",
            Auth = new RequestAuth
            {
                Kind = AuthKinds.AwsSigV4,
                Details = new AuthDetails
                {
                    AwsSecretAccessKey = "{{awsSecret}}",
                    AwsRegion = "{{region}}",
                    AwsService = "execute-api",
                    AwsAccessKeyId = "AKID",
                },
            },
        };

        var placeholders = VariableResolver.FindPlaceholders(request);

        Assert.Contains("awsSecret", placeholders);
        Assert.Contains("region", placeholders);
    }

    [Fact]
    public void Placeholders_in_fields_of_other_kinds_are_ignored()
    {
        var request = new ApiRequest
        {
            Url = "https://x.test",
            Auth = new RequestAuth { Kind = AuthKinds.Bearer, Token = "{{token}}", Details = new AuthDetails { AwsSecretAccessKey = "{{awsSecret}}" } },
        };

        Assert.DoesNotContain("awsSecret", VariableResolver.FindPlaceholders(request));
    }
}
