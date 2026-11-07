using Ricochet.Core;

namespace Ricochet.Core.Tests;

public class CurlGeneratorTests
{
    private static ApiRequest Request(string method = "GET", string url = "https://api.example.com/pets") => new()
    {
        Method = method,
        Url = url,
    };

    [Fact]
    public void A_get_request_with_no_extras_is_just_the_url()
    {
        Assert.Equal("curl 'https://api.example.com/pets'", CurlGenerator.Generate(Request(), []));
    }

    [Fact]
    public void Other_methods_are_named_with_X()
    {
        Assert.StartsWith("curl -X POST \\", CurlGenerator.Generate(Request("post"), []));
    }

    [Fact]
    public void Disabled_and_blank_headers_are_left_out()
    {
        var request = Request() with
        {
            Headers = [new KeyValue("Accept", "application/json"), new KeyValue("X-Off", "1", Enabled: false), new KeyValue("  ", "x")],
        };

        var text = CurlGenerator.Generate(request, []);

        Assert.Contains("-H 'Accept: application/json'", text);
        Assert.DoesNotContain("X-Off", text);
    }

    [Fact]
    public void A_body_gets_the_content_type_the_sender_would_use()
    {
        var text = CurlGenerator.Generate(Request("POST") with { Body = "{\"name\":\"Rex\"}" }, []);

        Assert.Contains("-H 'Content-Type: application/json'", text);
        Assert.Contains("--data-raw '{\"name\":\"Rex\"}'", text);
    }

    [Fact]
    public void An_explicit_content_type_is_not_duplicated()
    {
        var request = Request("POST") with
        {
            Body = "name=Rex",
            Headers = [new KeyValue("Content-Type", "application/x-www-form-urlencoded")],
        };

        var text = CurlGenerator.Generate(request, []);

        Assert.Equal(2, text.Split("Content-Type").Length);
    }

    [Fact]
    public void Single_quotes_in_values_are_escaped_for_the_shell()
    {
        Assert.Equal("curl 'https://api.example.com/it'\\''s'", CurlGenerator.Generate(Request(url: "https://api.example.com/it's"), []));
    }

    [Fact]
    public void Bearer_tokens_are_masked_unless_revealed()
    {
        var request = Request() with { Auth = new RequestAuth { Kind = AuthKinds.Bearer, Token = "tok-123456" } };

        var masked = CurlGenerator.Generate(request, []);
        var revealed = CurlGenerator.Generate(request, [], reveal: true);

        Assert.DoesNotContain("tok-123456", masked);
        Assert.Contains("Authorization: Bearer ***", masked);
        Assert.Contains("Authorization: Bearer tok-123456", revealed);
    }

    [Fact]
    public void Basic_credentials_are_masked_unless_revealed()
    {
        var request = Request() with { Auth = new RequestAuth { Kind = AuthKinds.Basic, Username = "amy", Password = "pw-secret" } };

        Assert.Contains("-u 'amy:***'", CurlGenerator.Generate(request, []));
        Assert.Contains("-u 'amy:pw-secret'", CurlGenerator.Generate(request, [], reveal: true));
    }

    [Fact]
    public void Secret_variable_values_given_by_the_caller_are_masked_everywhere()
    {
        var request = Request("GET", "https://api.example.com/pets?key=hidden-value") with
        {
            Headers = [new KeyValue("X-Trace", "hidden-value")],
        };

        var text = CurlGenerator.Generate(request, ["hidden-value"]);

        Assert.DoesNotContain("hidden-value", text);
        Assert.Contains("?key=***", text);
    }

    [Fact]
    public void An_api_key_in_the_query_becomes_part_of_the_url()
    {
        var request = Request() with
        {
            Auth = new RequestAuth { Kind = AuthKinds.ApiKey, KeyName = "api_key", KeyValue = "k e y", KeyInQuery = true },
        };

        var text = CurlGenerator.Generate(request, [], reveal: true);

        Assert.Contains("'https://api.example.com/pets?api_key=k%20e%20y'", text);
        Assert.DoesNotContain("-H", text);
    }

    [Fact]
    public void An_api_key_in_a_header_is_masked_unless_revealed()
    {
        var request = Request() with
        {
            Auth = new RequestAuth { Kind = AuthKinds.ApiKey, KeyName = "X-Api-Key", KeyValue = "abcdef123" },
        };

        Assert.Contains("-H 'X-Api-Key: ***'", CurlGenerator.Generate(request, []));
        Assert.Contains("-H 'X-Api-Key: abcdef123'", CurlGenerator.Generate(request, [], reveal: true));
    }
}
