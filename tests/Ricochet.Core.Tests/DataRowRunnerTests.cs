using System.Net;
using Ricochet.Core;

namespace Ricochet.Core.Tests;

public class DataRowRunnerTests
{
    // A fake sender: records each URL it is asked to send and answers 200.
    private sealed class Handler : HttpMessageHandler
    {
        public List<string> Urls { get; } = [];

        protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken)
        {
            Urls.Add(request.RequestUri!.ToString());
            return Task.FromResult(new HttpResponseMessage(HttpStatusCode.OK));
        }
    }

    private static Workspace Shop() => new()
    {
        Collections =
        [
            new Collection
            {
                Name = "Shop",
                Items =
                [
                    new CollectionItem
                    {
                        Name = "User",
                        Request = new ApiRequest { Name = "User", Method = "GET", Url = "https://api.example.test/users/{{name}}/{{region}}" },
                    },
                ],
            },
        ],
    };

    private static KeyValue[] Row(params (string Key, string Value)[] values) =>
        values.Select(v => new KeyValue(v.Key, v.Value)).ToArray();

    [Fact]
    public async Task Runs_the_collection_once_per_row_in_order_and_reports_each_run_separately()
    {
        var handler = new Handler();
        var runner = new DataRowRunner(new RequestSender(new HttpClient(handler)));
        IReadOnlyList<IReadOnlyList<KeyValue>> rows = [Row(("name", "ann"), ("region", "eu")), Row(("name", "bob"), ("region", "us"))];

        var reports = await runner.RunAsync(Shop(), new RunOptions { CollectionName = "Shop" }, rows);

        Assert.Equal(["https://api.example.test/users/ann/eu", "https://api.example.test/users/bob/us"], handler.Urls);
        Assert.Equal([1, 2], reports.Select(r => r.Row));
        Assert.All(reports, r =>
        {
            Assert.Single(r.Report.Results);
            Assert.True(r.Report.Passed);
        });
    }

    [Fact]
    public async Task A_row_value_overrides_a_given_override_with_the_same_name()
    {
        var handler = new Handler();
        var runner = new DataRowRunner(new RequestSender(new HttpClient(handler)));
        var options = new RunOptions { CollectionName = "Shop", Overrides = [new KeyValue("name", "given"), new KeyValue("region", "given")] };
        IReadOnlyList<IReadOnlyList<KeyValue>> rows = [Row(("name", "ann"))];

        await runner.RunAsync(Shop(), options, rows);

        Assert.Equal(["https://api.example.test/users/ann/given"], handler.Urls);
    }

    [Fact]
    public async Task A_row_missing_a_value_fails_only_its_own_run()
    {
        var handler = new Handler();
        var runner = new DataRowRunner(new RequestSender(new HttpClient(handler)));
        IReadOnlyList<IReadOnlyList<KeyValue>> rows = [Row(("name", "ann"), ("region", "eu")), Row(("name", "bob")), Row(("name", "cy"), ("region", "ap"))];

        var reports = await runner.RunAsync(Shop(), new RunOptions { CollectionName = "Shop" }, rows);

        Assert.Equal(3, reports.Count);
        Assert.True(reports[0].Report.Passed);
        Assert.False(reports[1].Report.Passed);
        Assert.Contains("region", reports[1].Report.Results[0].Error);
        Assert.True(reports[2].Report.Passed);
        Assert.Equal(["https://api.example.test/users/ann/eu", "https://api.example.test/users/cy/ap"], handler.Urls);
    }

    [Fact]
    public async Task Stop_on_first_failure_ends_the_data_run_at_the_failing_row()
    {
        var handler = new Handler();
        var runner = new DataRowRunner(new RequestSender(new HttpClient(handler)));
        IReadOnlyList<IReadOnlyList<KeyValue>> rows = [Row(("name", "ann"), ("region", "eu")), Row(("name", "bob")), Row(("name", "cy"), ("region", "ap"))];

        var reports = await runner.RunAsync(Shop(), new RunOptions { CollectionName = "Shop", StopOnFirstFailure = true }, rows);

        Assert.Equal([1, 2], reports.Select(r => r.Row));
        Assert.Single(handler.Urls);
    }

    [Fact]
    public async Task Each_row_reports_as_it_finishes()
    {
        var handler = new Handler();
        var runner = new DataRowRunner(new RequestSender(new HttpClient(handler)));
        IReadOnlyList<IReadOnlyList<KeyValue>> rows = [Row(("name", "ann"), ("region", "eu")), Row(("name", "bob"), ("region", "us"))];
        var seen = new List<int>();

        await runner.RunAsync(Shop(), new RunOptions { CollectionName = "Shop" }, rows, report => seen.Add(report.Row));

        Assert.Equal([1, 2], seen);
    }

    [Fact]
    public async Task An_unknown_collection_is_a_setup_error_not_a_row_failure()
    {
        var runner = new DataRowRunner(new RequestSender(new HttpClient(new Handler())));
        IReadOnlyList<IReadOnlyList<KeyValue>> rows = [Row(("name", "ann"), ("region", "eu"))];

        await Assert.ThrowsAsync<RunSetupException>(() => runner.RunAsync(Shop(), new RunOptions { CollectionName = "Nope" }, rows));
    }
}
