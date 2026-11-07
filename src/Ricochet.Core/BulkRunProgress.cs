namespace Ricochet.Core;

/// <summary>The numbers behind a bulk run's progress bar and status line.</summary>
public static class BulkRunProgress
{
    /// <summary>Whole percent of the ids that are processed, from 0 to 100. Zero when there are no ids.</summary>
    public static int Percent(int processed, int total)
    {
        if (total <= 0)
            return 0;

        return (int)Math.Clamp((long)processed * 100 / total, 0, 100);
    }

    /// <summary>A short line for the progress bar, such as "3 of 10 processed (30%)".</summary>
    public static string Describe(int processed, int total) =>
        total <= 0 ? "" : $"{processed} of {total} processed ({Percent(processed, total)}%)";
}
