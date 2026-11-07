using Ricochet.Core;

namespace Ricochet.Core.Tests;

public class DataRowReportsTests
{
    private static RequestRunResult Result(string collection, string path, bool passed) =>
        new(collection, path, "GET", 200, 5, passed ? [] : [new AssertionResult("status is 200", false, "got 500")], null, false);

    private static DataRowReport Row(int row, params RequestRunResult[] results) =>
        new(row, new RunReport(results, Stopped: false));

    [Fact]
    public void Each_request_keeps_its_row_number_so_the_same_request_in_two_rows_stays_two_results()
    {
        var rows = new[]
        {
            Row(1, Result("Shop", "/users", passed: true)),
            Row(2, Result("Shop", "/users", passed: true)),
        };

        var combined = DataRowReports.Combine(rows, stoppedEarly: false);

        Assert.Equal(["Shop (row 1)", "Shop (row 2)"], combined.Results.Select(r => r.Collection));
        Assert.True(combined.Passed);
    }

    [Fact]
    public void One_failing_row_makes_the_combined_report_fail()
    {
        var rows = new[]
        {
            Row(1, Result("Shop", "/users", passed: true)),
            Row(2, Result("Shop", "/users", passed: false)),
        };

        var combined = DataRowReports.Combine(rows, stoppedEarly: false);

        Assert.False(combined.Passed);
        Assert.Equal(1, combined.FailedRequests);
    }

    [Fact]
    public void Stopped_early_is_kept_when_the_data_run_stopped_or_a_row_did()
    {
        var rows = new[] { Row(1, Result("Shop", "/users", passed: false)) };

        Assert.True(DataRowReports.Combine(rows, stoppedEarly: true).Stopped);
        Assert.False(DataRowReports.Combine(rows, stoppedEarly: false).Stopped);
        Assert.True(DataRowReports.Combine([new DataRowReport(1, new RunReport([], Stopped: true))], stoppedEarly: false).Stopped);
    }
}
