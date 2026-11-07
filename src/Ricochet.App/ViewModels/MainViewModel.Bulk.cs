using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;
using Avalonia.Threading;
using CommunityToolkit.Mvvm.Input;
using Ricochet.Core;

namespace Ricochet.App.ViewModels;

/// <summary>The bulk run page: sends the request on screen once for each pasted id, and remembers where it stopped.</summary>
public sealed partial class MainViewModel
{
    private BulkRunner? _bulkRunner;
    private CancellationTokenSource? _bulkCts;

    private BulkRunner Bulk => _bulkRunner ??= new BulkRunner(_sender);

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(IsSidePageOpen))]
    private bool _isBulkPageOpen;

    [ObservableProperty] private string _bulkPlaceholder = BulkRunner.DefaultPlaceholder;
    [ObservableProperty] private string _bulkIdsText = "";
    [ObservableProperty] private string _bulkStatus = "";

    /// <summary>Ids processed so far in the run, and the total in the pasted list. Drive the progress bar.</summary>
    [ObservableProperty] private int _bulkProcessed;
    [ObservableProperty] private int _bulkTotal;

    /// <summary>The bar's maximum: the total, but never zero, so the bar is empty rather than undefined with no ids.</summary>
    [ObservableProperty] private int _bulkProgressMaximum = 1;
    [ObservableProperty] private string _bulkProgressText = "";

    /// <summary>True when a saved last id exists, so Resume has a place to continue from.</summary>
    [ObservableProperty] private bool _bulkHasResumePoint;

    /// <summary>One line saying what Start, Resume and Stop will do right now.</summary>
    [ObservableProperty] private string _bulkActionHint = "";
    [ObservableProperty] private string _bulkRequestName = "";

    /// <summary>How many ids are sent at once. Kept between 1 and 16 when a run starts.</summary>
    [ObservableProperty] private int _bulkThreads = 4;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(IsBulkRunning))]
    private bool _isBulkRunningFlag;

    public bool IsBulkRunning => IsBulkRunningFlag;

    partial void OnIsBulkRunningFlagChanged(bool value) => RefreshBulkActions();

    /// <summary>What the bulk run has sent this session for the page's request, one line per id.</summary>
    public ObservableCollection<string> BulkLog { get; } = [];

    // The request the log is showing, so a log is cleared when the page opens for a different request.
    private string? _bulkLogRequestId;

    /// <summary>The saved id key of the request on screen. Empty for a request that has never been saved.</summary>
    private string BulkKey => _currentNode?.Id ?? "";

    private BulkProgress? SavedBulkProgress() =>
        BulkKey.Length == 0 ? null : _store.BulkProgresses.FirstOrDefault(p => p.RequestId == BulkKey);

    [RelayCommand]
    private void OpenBulkPage()
    {
        if (_bulkLogRequestId != BulkKey && !IsBulkRunning)
        {
            BulkLog.Clear();
            _bulkLogRequestId = BulkKey;
        }

        var saved = SavedBulkProgress();
        if (BulkIdsText.Length == 0 && saved is not null)
            BulkIdsText = saved.Ids;

        BulkRequestName = CurrentRequest().Name is { Length: > 0 } name ? $"\"{name}\"" : "this request";
        BulkStatus = ProgressText(saved);
        SetBulkProgress(saved?.Processed ?? 0, BulkRunner.ParseIds(BulkIdsText).Count);

        IsEnvironmentsPageOpen = false;
        IsSettingsPageOpen = false;
        IsVariablesPageOpen = false;
        IsBulkPageOpen = true;
    }

    [RelayCommand]
    private void CloseBulkPage() => IsBulkPageOpen = false;

    /// <summary>Replaces the pasted ids with the ids from a file. Blank lines and surrounding spaces are dropped, as with paste.</summary>
    public void LoadBulkIds(string text, string fileName)
    {
        var ids = BulkRunner.ParseIds(text);
        BulkIdsText = string.Join('\n', ids);
        BulkStatus = ids.Count == 0
            ? $"{fileName} has no ids."
            : $"Loaded {ids.Count} id(s) from {fileName}.";
    }

    /// <summary>Removes the ids from the page. Saved progress and the log are kept, so Resume still knows where it stopped.</summary>
    [RelayCommand]
    private void ClearBulkIds()
    {
        BulkIdsText = "";
        BulkStatus = "Ids removed.";
    }

    /// <summary>Empties the log on screen. The saved bulk-log.txt keeps every run.</summary>
    [RelayCommand]
    private void ClearBulkLog() => BulkLog.Clear();

    [RelayCommand]
    private async Task StartBulkAsync()
    {
        var ids = BulkRunner.ParseIds(BulkIdsText);
        if (ids.Count == 0)
        {
            BulkStatus = "Paste at least one id.";
            return;
        }

        // Starting over forgets the last id, so Resume begins from the first.
        SaveBulkProgress(ids, lastId: null, processed: 0);
        await RunBulkAsync(0, ids);
    }

    [RelayCommand]
    private async Task ResumeBulkAsync()
    {
        var ids = BulkRunner.ParseIds(BulkIdsText);
        var saved = SavedBulkProgress();
        if (ids.Count == 0)
        {
            BulkStatus = "Paste the ids, then resume.";
            return;
        }

        // Resume after the last id that was processed. If that id isn't in the list, start from the first.
        var index = BulkRunner.ResumeIndex(ids, saved?.LastId);
        if (saved?.LastId is { } missing && !ids.Contains(missing))
            BulkStatus = $"The last id, {missing}, is not in this list, so the run starts from the first id.";

        await RunBulkAsync(index, ids);
    }

    [RelayCommand]
    private void StopBulk()
    {
        _bulkCts?.Cancel();
        BulkStatus = "Stopping after the current id…";
    }

    private async Task RunBulkAsync(int start, IReadOnlyList<string> ids)
    {
        if (IsBulkRunning)
            return;

        if (start >= ids.Count)
        {
            BulkStatus = "Every id has already been processed.";
            return;
        }

        var template = CurrentRequest();
        var collection = (CurrentCollection ?? _currentNode?.Root)?.ToCollection() ?? new Collection();
        var environment = HasActiveEnvironment ? ActiveEnvironment.ToEnvironment() : null;
        var workspace = Snapshot();
        var dotEnv = DotEnv.Read(_store.FolderPath);
        var placeholder = string.IsNullOrWhiteSpace(BulkPlaceholder) ? BulkRunner.DefaultPlaceholder : BulkPlaceholder.Trim();

        // Ids can finish out of order, so "processed" means the leading run of finished ids. Resume starts after that
        // run, which re-sends any later id that finished early. That is safe, and nothing is skipped.
        // Each run starts with an empty log, so its lines aren't mixed with an earlier run's. The saved file keeps every run, with a header.
        BulkLog.Clear();
        var header = $"--- Run from {ids[start]}, {Math.Clamp(BulkThreads, 1, 16)} thread(s), {ids.Count - start} id(s), {DateTime.Now:yyyy-MM-dd HH:mm:ss} ---";
        BulkLog.Add(header);
        _store.AppendBulkLog($"{BulkKey} {header}");
        foreach (var notice in DotEnvNotice(dotEnv).Split('\n', StringSplitOptions.RemoveEmptyEntries))
            BulkLog.Add($"Warning: {notice}");

        var finishedFlags = new bool[ids.Count];
        var processed = start;
        SetBulkProgress(processed, ids.Count);
        // Calls actually sent in this run, and how many got a 2xx. Ids that were not sent (missing variable) don't count.
        var callsSent = 0;
        var callsOk = 0;
        var frontier = start;
        var threads = Math.Clamp(BulkThreads, 1, 16);
        var cts = new CancellationTokenSource();
        _bulkCts = cts;
        IsBulkRunningFlag = true;
        BulkStatus = $"Running from {ids[start]} on {threads} {(threads == 1 ? "thread" : "threads")}…";

        try
        {
            var finished = await Bulk.RunAsync(workspace, collection, environment, template, placeholder,
                ids.Skip(start).ToList(), result => Dispatcher.UIThread.Post(() =>
                {
                    BulkLog.Add(result.LogLine);
                    _store.AppendBulkLog($"{BulkKey} {result.LogLine}");

                    finishedFlags[start + result.Position] = true;
                    processed++;
                    if (result.StatusCode is not null)
                        callsSent++;
                    if (result.Succeeded)
                        callsOk++;
                    while (frontier < ids.Count && finishedFlags[frontier])
                        frontier++;

                    var lastProcessed = frontier > 0 ? ids[frontier - 1] : null;
                    SaveBulkProgress(ids, lastProcessed, processed);
                    SetBulkProgress(processed, ids.Count);
                    BulkStatus = $"{processed} of {ids.Count} processed. Last id: {lastProcessed ?? "none"}. {CallsText(callsSent, callsOk)}";
                }), cts.Token, threads, overrides: dotEnv.Variables, itemId: _currentNode?.Id);

            var lastId = frontier > 0 ? ids[frontier - 1] : null;
            SetBulkProgress(processed, ids.Count);
            BulkStatus = finished
                ? $"Finished. {processed} of {ids.Count} processed. {CallsText(callsSent, callsOk)}"
                : $"Stopped after {lastId ?? "nothing"}. Resume to continue from there. {CallsText(callsSent, callsOk)}";
        }
        finally
        {
            IsBulkRunningFlag = false;
            _bulkCts = null;
            cts.Dispose();
        }
    }

    private void SaveBulkProgress(IReadOnlyList<string> ids, string? lastId, int processed)
    {
        if (BulkKey.Length == 0)
            return;

        _store.SetBulkProgress(new BulkProgress
        {
            RequestId = BulkKey,
            Ids = string.Join('\n', ids),
            LastId = lastId,
            Processed = processed,
        });
        BulkHasResumePoint = lastId is not null;
        RefreshBulkActions();
    }

    private void SetBulkProgress(int processed, int total)
    {
        BulkTotal = Math.Max(total, 0);
        BulkProgressMaximum = Math.Max(BulkTotal, 1);
        BulkProcessed = Math.Clamp(processed, 0, BulkTotal);
        BulkProgressText = BulkRunProgress.Describe(BulkProcessed, BulkTotal);
    }

    /// <summary>Works out the action hint from the run state and the saved progress. Called whenever either changes.</summary>
    private void RefreshBulkActions()
    {
        if (IsBulkRunning)
        {
            BulkActionHint = "Running. Stop finishes the ids already in flight, then the run ends.";
            return;
        }

        BulkActionHint = BulkHasResumePoint && SavedBulkProgress()?.LastId is { } last
            ? $"Resume continues after {last}. Start sends every id from the first one."
            : "Start sends every id from the first one.";
    }

    private static string CallsText(int sent, int ok) =>
        $"Responses: {sent} ({ok} OK, {sent - ok} failed).";

    private string ProgressText(BulkProgress? saved)
    {
        BulkHasResumePoint = saved?.LastId is not null;
        RefreshBulkActions();
        return saved is null
        ? "No ids processed yet for this request."
        : $"Last id processed: {saved.LastId ?? "none"}. {saved.Processed} processed so far.";
    }
}
