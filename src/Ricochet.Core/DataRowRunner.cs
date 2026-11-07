namespace Ricochet.Core;

/// <summary>The report for one row of a data file, from the run made with that row's values.</summary>
/// <param name="Row">The row's place in the data file, counted from 1 over the rows that hold values.</param>
public sealed record DataRowReport(int Row, RunReport Report);

/// <summary>
/// Runs a Collection once per row of a data file (ADR 0018). Each row's values are given to its run as overrides,
/// so every run starts fresh: captures and results never carry over from one row to the next.
/// Reads the Workspace and never changes it.
/// </summary>
public sealed class DataRowRunner(RequestSender sender)
{
    /// <exception cref="RunSetupException">The run could not be set up, such as an unknown Collection name.</exception>
    public async Task<IReadOnlyList<DataRowReport>> RunAsync(
        Workspace workspace,
        RunOptions options,
        IReadOnlyList<IReadOnlyList<KeyValue>> rows,
        Action<DataRowReport>? onRow = null,
        CancellationToken cancellationToken = default)
    {
        var runner = new CollectionRunner(sender);
        var reports = new List<DataRowReport>();

        for (var index = 0; index < rows.Count; index++)
        {
            cancellationToken.ThrowIfCancellationRequested();

            // Row values come after the given overrides, so a row wins when both name the same Variable.
            var rowOptions = options with { Overrides = [.. options.Overrides, .. rows[index]] };
            var report = await runner.RunAsync(workspace, rowOptions, cancellationToken: cancellationToken);

            var entry = new DataRowReport(index + 1, report);
            reports.Add(entry);
            onRow?.Invoke(entry);

            // Stop on first failure stops the whole data run, not just the row that failed.
            if (options.StopOnFirstFailure && !report.Passed)
                break;
        }

        return reports;
    }
}
