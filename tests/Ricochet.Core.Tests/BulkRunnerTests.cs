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
    public async Task Stop_pressed_during_an_id_lets_that_id_finish()
    {
        using var cancel = new CancellationTokenSource();
        var handler = new Handler(_ =>
        {
            cancel.Cancel();
            return new HttpResponseMessage(HttpStatusCode.OK);
        });
        var runner = new BulkRunner(new RequestSender(new HttpClient(handler)));
        var results = new List<BulkItemResult>();

        var finished = await runner.RunAsync(Empty(), Plain(), null, Template(), "id", ["p1", "p2"], results.Add, cancel.Token);

        Assert.False(finished);
        Assert.Equal(["p1"], results.Select(r => r.Id));
        Assert.True(results[0].Succeeded);
        Assert.Single(handler.Urls);
    }

    private sealed class OverlapHandler : HttpMessageHandler
    {
        private int _current;
        public int MaxConcurrent { get; private set; }

        protected override async Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken)
        {
            var now = Interlocked.Increment(ref _current);
            lock (this)
                MaxConcurrent = Math.Max(MaxConcurrent, now);
            await Task.Delay(50, cancellationToken);
            Interlocked.Decrement(ref _current);
            return new HttpResponseMessage(HttpStatusCode.OK);
        }
    }

    [Fact]
    public async Task Parallel_run_never_exceeds_the_thread_count_and_reports_every_id()
    {
        var handler = new OverlapHandler();
        var runner = new BulkRunner(new RequestSender(new HttpClient(handler)));
        var results = new List<BulkItemResult>();
        string[] ids = ["p1", "p2", "p3", "p4", "p5", "p6", "p7", "p8"];

        var finished = await runner.RunAsync(Empty(), Plain(), null, Template(), "id", ids, results.Add, maxParallel: 3);

        Assert.True(finished);
        Assert.Equal(3, handler.MaxConcurrent);
        Assert.Equal(ids.OrderBy(id => id), results.Select(r => r.Id).OrderBy(id => id));
        Assert.Equal(Enumerable.Range(0, ids.Length), results.Select(r => r.Position).OrderBy(p => p));
    }

    [Fact]
    public async Task One_thread_sends_one_id_at_a_time()
    {
        var handler = new OverlapHandler();
        var runner = new BulkRunner(new RequestSender(new HttpClient(handler)));

        await runner.RunAsync(Empty(), Plain(), null, Template(), "id", ["p1", "p2", "p3"], _ => { }, maxParallel: 1);

        Assert.Equal(1, handler.MaxConcurrent);
    }

    [Fact]
    public async Task Stopping_a_parallel_run_starts_no_new_ids_but_finishes_those_in_flight()
    {
        var handler = new OverlapHandler();
        var runner = new BulkRunner(new RequestSender(new HttpClient(handler)));
        using var cancel = new CancellationTokenSource(TimeSpan.FromMilliseconds(120));
        var results = new List<BulkItemResult>();
        string[] ids = Enumerable.Range(1, 40).Select(i => $"p{i}").ToArray();

        var finished = await runner.RunAsync(Empty(), Plain(), null, Template(), "id", ids, results.Add, cancel.Token, maxParallel: 4);

        Assert.False(finished);
        Assert.True(results.Count < ids.Length);
        Assert.All(results, r => Assert.True(r.Succeeded));
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

    [Fact]
    public async Task A_given_override_is_masked_in_the_detail_of_a_failed_id_like_a_secret()
    {
        // RunOptions.Overrides are treated as Secrets in reports, so a value given for a bulk run is masked too.
        var handler = new Handler(_ => new HttpResponseMessage(HttpStatusCode.Unauthorized) { Content = new StringContent("key ovr-val-456 was rejected") });
        var runner = new BulkRunner(new RequestSender(new HttpClient(handler)));
        var results = new List<BulkItemResult>();

        await runner.RunAsync(Empty(), Plain(), null, Template(), "id", ["p1"], results.Add, overrides: [new KeyValue("key", "ovr-val-456")]);

        Assert.DoesNotContain("ovr-val-456", results[0].Detail);
        Assert.Contains("***", results[0].Detail);
    }
}
