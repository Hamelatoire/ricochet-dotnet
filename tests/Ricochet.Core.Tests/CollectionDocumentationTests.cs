using Ricochet.Core;

namespace Ricochet.Core.Tests;

public class CollectionDocumentationTests
{
    private static CollectionItem Request(string name, string url, Action<ApiRequest>? configure = null)
    {
        var request = new ApiRequest { Name = name, Url = url };
        configure?.Invoke(request);
        return new CollectionItem { Name = name, Request = request };
    }

    private static CollectionItem Folder(string name, params CollectionItem[] children) =>
        new() { Name = name, Children = children };

    private static Collection Single(params CollectionItem[] items) => new() { Name = "Pets", Items = items };

    [Fact]
    public void Escapes_names_urls_headers_and_bodies()
    {
        var html = CollectionDocumentation.Render(Single(
            Request("<script>alert(1)</script>", "https://x.test/?q=\"a\"&b='c'", r =>
            {
                r.Headers = [new KeyValue("X-Note", "<img src=x onerror=alert(1)>")];
                r.Body = "{\"a\": \"</pre><script>\"}";
            })));

        Assert.DoesNotContain("<script", html);
        Assert.DoesNotContain("<img", html);
        Assert.Contains("&lt;script&gt;alert(1)&lt;/script&gt;", html);
        Assert.Contains("&lt;img src=x onerror=alert(1)&gt;", html);
        Assert.Contains("&lt;/pre&gt;&lt;script&gt;", html);
        Assert.Contains("q=&quot;a&quot;&amp;b=&#39;c&#39;", html);
    }

    [Fact]
    public void Escapes_the_collection_name()
    {
        var html = CollectionDocumentation.Render(new Collection { Name = "A & <B>" });

        Assert.Contains("<title>A &amp; &lt;B&gt;</title>", html);
        Assert.Contains("<h1>A &amp; &lt;B&gt;</h1>", html);
    }

    [Fact]
    public void Masks_secret_variables()
    {
        var html = CollectionDocumentation.Render(new Collection
        {
            Name = "Pets",
            Variables =
            [
                new KeyValue("baseUrl", "https://api.test"),
                new KeyValue("apiSecret", "hunter2", IsSecret: true),
                new KeyValue("emptySecret", "", IsSecret: true),
                new KeyValue("disabledSecret", "leaked", Enabled: false, IsSecret: true),
            ],
        });

        Assert.DoesNotContain("hunter2", html);
        Assert.DoesNotContain("leaked", html);
        Assert.Contains("https://api.test", html);
        Assert.Contains("apiSecret", html);
        Assert.Contains(CollectionDocumentation.Masked, html);
    }

    [Fact]
    public void Masks_literal_credentials_and_keeps_placeholders()
    {
        var html = CollectionDocumentation.Render(Single(
            Request("Login", "https://x.test/", r => r.Auth = new RequestAuth
            {
                Kind = AuthKinds.Basic,
                Username = "alice",
                Password = "s3cret-pass",
            }),
            Request("Bearer", "https://x.test/", r => r.Auth = new RequestAuth
            {
                Kind = AuthKinds.Bearer,
                Token = "{{apiToken}}",
            }),
            Request("ApiKey", "https://x.test/", r => r.Auth = new RequestAuth
            {
                Kind = AuthKinds.ApiKey,
                KeyName = "X-Key",
                KeyValue = "key-abc-123",
            })));

        Assert.DoesNotContain("s3cret-pass", html);
        Assert.DoesNotContain("key-abc-123", html);
        Assert.Contains("alice", html);
        Assert.Contains("{{apiToken}}", html);
        Assert.Contains("X-Key", html);
    }

    [Fact]
    public void Masks_sensitive_headers_and_secret_header_values()
    {
        var html = CollectionDocumentation.Render(Single(Request("Get", "https://x.test/", r =>
        {
            r.Headers =
            [
                new KeyValue("Authorization", "Bearer abc"),
                new KeyValue("Cookie", "{{session}}"),
                new KeyValue("X-Custom", "visible-value"),
                new KeyValue("X-Hidden", "secret-header", IsSecret: true),
                new KeyValue("X-Api-Key", "off", Enabled: false),
            ];
        })));

        Assert.DoesNotContain("Bearer abc", html);
        Assert.DoesNotContain("secret-header", html);
        Assert.DoesNotContain("X-Api-Key", html);
        Assert.Contains("{{session}}", html);
        Assert.Contains("visible-value", html);
    }

    [Fact]
    public void Masks_sensitive_query_parameters_in_the_url()
    {
        var html = CollectionDocumentation.Render(Single(
            Request("Search", "https://x.test/items?api_key=abc123&page=2&token={{ok}}")));

        Assert.DoesNotContain("abc123", html);
        Assert.Contains("api_key=***", html);
        Assert.Contains("page=2", html);
        Assert.Contains("token={{ok}}", html);
    }

    [Fact]
    public void Nests_requests_inside_their_folders()
    {
        var html = CollectionDocumentation.Render(Single(
            Folder("Outer",
                Folder("Inner", Request("Deep request", "https://x.test/deep"))),
            Request("Top request", "https://x.test/top")));

        var deepAt = html.IndexOf("Deep request", StringComparison.Ordinal);
        var topAt = html.IndexOf("Top request", StringComparison.Ordinal);

        Assert.Equal(2, CountOpenSections(html, deepAt));
        Assert.Equal(0, CountOpenSections(html, topAt));
        Assert.Contains("<h2>Outer</h2>", html);
        Assert.Contains("<h3>Inner</h3>", html);
        Assert.Contains("<h4><span class=\"method\">GET</span> Deep request</h4>", html);
    }

    [Fact]
    public void Clamps_heading_levels_for_deep_folders()
    {
        CollectionItem leaf = Request("Leaf", "https://x.test/");
        for (var i = 0; i < 6; i++)
            leaf = Folder($"Level{i}", leaf);

        var html = CollectionDocumentation.Render(Single(leaf));

        Assert.Contains("<h2>Level5</h2>", html);
        Assert.Contains("<h6>Level1</h6>", html);
        Assert.Contains("<h6>Level0</h6>", html);
        Assert.Contains("<h6><span class=\"method\">GET</span> Leaf</h6>", html);
    }

    [Fact]
    public void Leaves_out_hidden_folders_and_requests()
    {
        var hiddenRequest = Request("Hidden request", "https://x.test/hidden");
        var hiddenFolder = Folder("Hidden folder", Request("Inside hidden", "https://x.test/inside"));
        var collection = new Collection
        {
            Name = "Pets",
            Items = [Request("Shown", "https://x.test/shown"), hiddenRequest, hiddenFolder],
            HiddenItemIds = [hiddenRequest.Id, hiddenFolder.Id],
        };

        var html = CollectionDocumentation.Render(collection);

        Assert.Contains("Shown", html);
        Assert.DoesNotContain("Hidden request", html);
        Assert.DoesNotContain("Hidden folder", html);
        Assert.DoesNotContain("Inside hidden", html);
    }

    [Fact]
    public void Page_has_no_scripts_external_resources_or_network_calls()
    {
        var html = CollectionDocumentation.Render(Single(
            Folder("Folder", Request("Get", "https://api.example.test/v1/pets"))));

        Assert.DoesNotContain("<script", html, StringComparison.OrdinalIgnoreCase);
        Assert.DoesNotContain("<link", html, StringComparison.OrdinalIgnoreCase);
        Assert.DoesNotContain("<img", html, StringComparison.OrdinalIgnoreCase);
        Assert.DoesNotContain("<iframe", html, StringComparison.OrdinalIgnoreCase);
        Assert.DoesNotContain("@import", html, StringComparison.OrdinalIgnoreCase);
        Assert.DoesNotContain("url(", html, StringComparison.OrdinalIgnoreCase);
        Assert.DoesNotContain(" src=", html, StringComparison.OrdinalIgnoreCase);
        Assert.DoesNotContain(" href=", html, StringComparison.OrdinalIgnoreCase);
        Assert.Equal(1, html.Split("<style>").Length - 1);
        Assert.StartsWith("<!DOCTYPE html>", html, StringComparison.Ordinal);
    }

    [Fact]
    public void Rendering_is_deterministic()
    {
        var collection = Single(Folder("F", Request("R", "https://x.test/")));

        Assert.Equal(CollectionDocumentation.Render(collection), CollectionDocumentation.Render(collection));
    }

    /// <summary>How many folder sections are open at <paramref name="position"/>.</summary>
    private static int CountOpenSections(string html, int position)
    {
        var before = html[..position];
        return CountOccurrences(before, "<section class=\"folder\">") - CountOccurrences(before, "</section>");
    }

    private static int CountOccurrences(string text, string needle)
    {
        var count = 0;
        var index = 0;
        while ((index = text.IndexOf(needle, index, StringComparison.Ordinal)) >= 0)
        {
            count++;
            index += needle.Length;
        }

        return count;
    }
}
