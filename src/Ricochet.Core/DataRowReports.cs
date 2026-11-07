namespace Ricochet.Core;

/// <summary>Joins the reports of a data run into one report, so one xUnit file and one exit code can cover every row (ADR 0018).</summary>
public static class DataRowReports
{
    /// <summary>
    /// Each Request's Collection is given its row number, so the same Request in two rows stays two results.
    /// The combined report is stopped when any row stopped, or when <paramref name="stoppedEarly"/> says the data run did.
    /// </summary>
    public static RunReport Combine(IReadOnlyList<DataRowReport> rows, bool stoppedEarly) =>
        new(
            rows.SelectMany(row => row.Report.Results.Select(result => result with { Collection = $"{result.Collection} (row {row.Row})" })).ToList(),
            stoppedEarly || rows.Any(row => row.Report.Stopped));
}
