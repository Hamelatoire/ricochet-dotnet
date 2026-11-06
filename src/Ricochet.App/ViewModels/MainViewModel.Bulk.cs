using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;
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
    [ObservableProperty] private string _bulkRequestName = "";

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(IsBulkRunning))]
    private bool _isBulkRunningFlag;

    public bool IsBulkRunning => IsBulkRunningFlag;

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

        IsEnvironmentsPageOpen = false;
        IsSettingsPageOpen = false;
        IsVariablesPageOpen = false;
        IsBulkPageOpen = true;
    }

    [RelayCommand]
    private void CloseBulkPage() => IsBulkPageOpen = false;

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
        var index = saved?.LastId is { } last ? ids.ToList().IndexOf(last) + 1 : 0;
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
        var placeholder = string.IsNullOrWhiteSpace(BulkPlaceholder) ? BulkRunner.DefaultPlaceholder : BulkPlaceholder.Trim();

        var processed = start;
        string? last = start > 0 ? ids[start - 1] : null;
        var cts = new CancellationTokenSource();
        _bulkCts = cts;
        IsBulkRunningFlag = true;
        BulkStatus = $"Running from {ids[start]}…";

        try
        {
            var finished = await Bulk.RunAsync(workspace, collection, environment, template, placeholder,
                ids.Skip(start).ToList(), result =>
                {
                    processed++;
                    last = result.Id;
                    BulkLog.Add(result.LogLine);
                    _store.AppendBulkLog($"{BulkKey} {result.LogLine}");
                    SaveBulkProgress(ids, last, processed);
                    BulkStatus = $"{processed} of {ids.Count} processed. Last id: {last}";
                }, cts.Token);

            BulkStatus = finished
                ? $"Finished. {processed} of {ids.Count} processed."
                : $"Stopped after {last}. Resume to continue from there.";
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
    }

    private static string ProgressText(BulkProgress? saved) => saved is null
        ? "No ids processed yet for this request."
        : $"Last id processed: {saved.LastId ?? "none"}. {saved.Processed} processed so far.";
}
