using System.Text;
using Ricochet.Core;

namespace Ricochet.Core.Tests;

public sealed class ResponseComparisonTests
{
    private static ApiResponse Response(string body, string? mediaType = "application/json", IEnumerable<KeyValue>? headers = null) => new(
        200,
        "OK",
        (headers ?? []).ToList(),
        Encoding.UTF8.GetBytes(body),
        mediaType,
        null,
        TimeSpan.FromMilliseconds(5),
        false);

    [Fact]
    public void Identical_bodies_are_all_same_rows()
    {
        var result = ResponseComparer.Compare(Response("a\nb"), Response("a\nb"));

        Assert.All(result.Body, row => Assert.Equal(DiffKind.Same, row.Kind));
        Assert.Equal(2, result.Body.Count);
        Assert.Null(result.BodyNote);
    }

    [Fact]
    public void A_changed_line_is_paired_on_both_sides()
    {
        var result = ResponseComparer.Compare(Response("one\ntwo\nthree", "text/plain"), Response("one\n2\nthree", "text/plain"));

        var changed = Assert.Single(result.Body, row => row.Kind != DiffKind.Same);
        Assert.Equal(DiffKind.Changed, changed.Kind);
        Assert.Equal("two", changed.Previous);
        Assert.Equal(2, changed.PreviousNumber);
        Assert.Equal("2", changed.Current);
        Assert.Equal(2, changed.CurrentNumber);
    }

    [Fact]
    public void Added_and_removed_lines_have_no_counterpart()
    {
        var result = ResponseComparer.Compare(Response("a\nb", "text/plain"), Response("a\nc\nd", "text/plain"));

        var removed = Assert.Single(result.Body, row => row.Kind == DiffKind.Changed);
        Assert.Equal("b", removed.Previous);
        var added = Assert.Single(result.Body, row => row.Kind == DiffKind.Added);
        Assert.Equal("d", added.Current);
        Assert.Null(added.Previous);
        Assert.Null(added.PreviousNumber);
    }

    [Fact]
    public void Whitespace_inside_json_does_not_make_lines_differ()
    {
        // Both bodies pretty-print to the same text, so no row should differ.
        var result = ResponseComparer.Compare(Response("{\"a\":1,\"b\":2}"), Response("{ \"a\" : 1,   \"b\": 2 }"));

        Assert.All(result.Body, row => Assert.Equal(DiffKind.Same, row.Kind));
    }

    [Fact]
    public void Binary_bodies_are_not_compared()
    {
        var binary = new ApiResponse(200, "OK", [], [0, 1, 2, 3, 0, 255], "application/octet-stream", null, TimeSpan.Zero, false);

        var result = ResponseComparer.Compare(binary, Response("text"));

        Assert.Empty(result.Body);
        Assert.NotNull(result.BodyNote);
    }

    [Fact]
    public void Headers_are_matched_by_name_ignoring_case_and_changes_are_marked()
    {
        var before = Response("x", headers: [new KeyValue("X-Limit", "10"), new KeyValue("Server", "a")]);
        var after = Response("x", headers: [new KeyValue("x-limit", "20"), new KeyValue("X-New", "yes")]);

        var result = ResponseComparer.Compare(before, after);

        var limit = result.Headers.Single(h => h.Name.Equals("X-Limit", StringComparison.OrdinalIgnoreCase));
        Assert.True(limit.Changed);
        Assert.Equal("10", limit.Previous);
        Assert.Equal("20", limit.Current);
        var server = result.Headers.Single(h => h.Name == "Server");
        Assert.Null(server.Current);
        Assert.True(server.Changed);
        var added = result.Headers.Single(h => h.Name == "X-New");
        Assert.Null(added.Previous);
        Assert.True(added.Changed);
    }

    [Fact]
    public void Repeated_headers_are_joined_before_comparing()
    {
        var before = Response("x", headers: [new KeyValue("Set-Cookie", "a=1"), new KeyValue("Set-Cookie", "b=2")]);
        var after = Response("x", headers: [new KeyValue("Set-Cookie", "a=1, b=2")]);

        var result = ResponseComparer.Compare(before, after);

        Assert.False(Assert.Single(result.Headers).Changed);
    }

    [Fact]
    public void Secret_values_are_masked_in_body_and_headers()
    {
        const string secret = "sk-live-9876543210";
        var before = Response($"token={secret}", "text/plain", [new KeyValue("X-Echo", secret)]);
        var after = Response("token=other", "text/plain", [new KeyValue("X-Echo", "other")]);

        var result = ResponseComparer.Compare(before, after, [secret]);

        var text = string.Join("\n", result.Body.SelectMany(row => new[] { row.Previous, row.Current }));
        Assert.DoesNotContain(secret, text);
        Assert.DoesNotContain(secret, result.Headers.SelectMany(h => new[] { h.Previous, h.Current }));
        Assert.Equal(SecretRules.HiddenValue, result.Headers.Single().Previous);
    }

    [Fact]
    public void Sensitive_headers_are_hidden_and_never_counted_as_changed()
    {
        var before = Response("x", headers: [new KeyValue("Authorization", "Bearer one")]);
        var after = Response("x", headers: [new KeyValue("Authorization", "Bearer two")]);

        var result = ResponseComparer.Compare(before, after);

        var header = Assert.Single(result.Headers);
        Assert.True(header.Hidden);
        Assert.False(header.Changed);
        Assert.Equal(SecretRules.HiddenValue, header.Previous);
        Assert.Equal(SecretRules.HiddenValue, header.Current);
    }
}
