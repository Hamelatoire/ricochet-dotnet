using System.Net;
using System.Text;
using Ricochet.Core;

namespace Ricochet.Core.Tests;

public class BulkRunnerTests
{
    private sealed class Handler(Func<HttpRequestMessage, HttpResponseMessage> respond) : HttpMessageHandler
    {
        public List<string> Urls { get; } = [];

        protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken)
        {
            Urls.Add(request.RequestUri!.ToString());
            return Task.FromResult(respond(request));
        }
    }

    private static ApiRequest Template() => new()
    {
        Name = "Sync",
        Method = "GET",
        Url = "https://api.example.test/products/{{id}}/sync",
    };

    private static Workspace Empty() => new();

    private static Collection Plain() => new() { Name = "Shop" };

    [Fact]
    public void Ids_are_split_per_line_and_blank_lines_are_dropped()
    {
        var ids = BulkRunner.ParseIds("  a1 \r\n\r\nb2\n   \nc3");

        Assert.Equal(["a1", "b2", "c3"], ids);
    }

    [Fact]
    public async Task Sends_one_request_per_id_in_order_and_reports_each_result()
    {
        var handler = new Handler(request => request.RequestUri!.AbsolutePath.Contains("bad")
            ? new HttpResponseMessage(HttpStatusCode.NotFound) { Content = new StringContent("no such product") }
            : new HttpResponseMessage(HttpStatusCode.OK));
        var runner = new BulkRunner(new RequestSender(new HttpClient(handler)));
        var results = new List<BulkItemResult>();

        var finished = await runner.RunAsync(Empty(), Plain(), null, Template(), "id", ["p1", "bad", "p3"], results.Add);

        Assert.True(finished);
        Assert.Equal(["https://api.example.test/products/p1/sync", "https://api.example.test/products/bad/sync", "https://api.example.test/products/p3/sync"], handler.Urls);
        Assert.Equal(["p1", "bad", "p3"], results.Select(r => r.Id));
        Assert.Equal("OK  p1  [200]", results[0].LogLine);
        Assert.Equal("ERR bad  [404] no such product", results[1].LogLine);
    }

    [Fact]
    public async Task Stopping_after_an_id_sends_no_more()
    {
        var handler = new Handler(_ => new HttpResponseMessage(HttpStatusCode.OK));
        var runner = new BulkRunner(new RequestSender(new HttpClient(handler)));
        using var cancel = new CancellationTokenSource();
        var results = new List<BulkItemResult>();

        var finished = await runner.RunAsync(Empty(), Plain(), null, Template(), "id", ["p1", "p2", "p3"], result =>
        {
            results.Add(result);
            cancel.Cancel();
        }, cancel.Token);

        Assert.False(finished);
        Assert.Equal(["p1"], results.Select(r => r.Id));
        Assert.Single(handler.Urls);
    }

    [Fact]
    public async Task An_id_with_a_missing_variable_is_not_sent()
    {
        var handler = new Handler(_ => new HttpResponseMessage(HttpStatusCode.OK));
        var runner = new BulkRunner(new RequestSender(new HttpClient(handler)));
        var template = Template() with { Url = "https://api.example.test/{{id}}/{{region}}" };
        var results = new List<BulkItemResult>();

        await runner.RunAsync(Empty(), Plain(), null, template, "id", ["p1"], results.Add);

        Assert.Empty(handler.Urls);
        Assert.False(results[0].Succeeded);
        Assert.Contains("region", results[0].Detail);
    }
}
