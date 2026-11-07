using System.Collections.Generic;
using System.Collections.ObjectModel;
using System.Collections.Specialized;
using System.Globalization;
using Avalonia.Threading;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using Ricochet.Core;

namespace Ricochet.App.ViewModels;

public sealed record AuthOption(string Kind, string Label);

public sealed partial class MainViewModel : ObservableObject
{
    private readonly RequestSender _sender;
    private readonly WorkspaceStore _store;
    private readonly EnvironmentViewModel _noEnvironment;
    private CancellationTokenSource? _sendCts;
    private Task _pendingSave = Task.CompletedTask;
    private TreeNodeViewModel? _currentNode;
    private ISecretVault _vault;
    private IReadOnlyDictionary<string, string> _storedSecrets;
    private IDisposable? _folderWatcher;

    // The last response and what produced it, kept so it can be saved as an example (ADR 0023).
    private (ApiRequest Sent, IReadOnlyList<KeyValue> Variables, IReadOnlyList<KeyValue> Captured, ApiResponse Response)? _lastExchange;

    public MainViewModel(RequestSender sender, WorkspaceStore store, ISecretVault vault)
    {
        _sender = sender;
        _store = store;
        _vault = vault;
        _storedSecrets = LoadSecrets();
        _settings = store.Settings;
        ApplySettings();

        var loaded = store.Load();
        _themeChoice = ThemeSettings.ChoiceName(store.Theme);
        var workspace = WorkspaceSecrets.Merge(loaded.Workspace, WorkspaceSecrets.ForWorkspace(_storedSecrets, store.WorkspaceId));
        Collections = new ObservableCollection<TreeNodeViewModel>(
            workspace.Collections.Select(c => TreeNodeViewModel.FromCollection(c, DeleteNode, RenameNode, AddFolder)));
        History = new ObservableCollection<HistoryEntry>(workspace.History.Select(WorkspaceSecrets.SanitizeHistory));
        Globals.Load(workspace.Globals);
        Headers.Items.CollectionChanged += (_, _) => OnPropertyChanged(nameof(HeadersTabTitle));
        Assertions.CollectionChanged += (_, _) => OnPropertyChanged(nameof(TestsTabTitle));
        Captures.CollectionChanged += (_, _) => OnPropertyChanged(nameof(TestsTabTitle));
        Examples.CollectionChanged += (_, _) =>
        {
            OnPropertyChanged(nameof(ExamplesTabTitle));
            OnPropertyChanged(nameof(HasExamples));
        };
        TestResults.CollectionChanged += (_, _) =>
        {
            OnPropertyChanged(nameof(ResponseTestsTabTitle));
            OnPropertyChanged(nameof(HasTestResults));
        };

        ShowIssues(loaded.Issues);
        RefreshPlainTextCopies();


        _noEnvironment = new EnvironmentViewModel(new ApiEnvironment { Id = "", Name = "No environment" }, _ => { });
        Environments = [_noEnvironment];
        foreach (var environment in workspace.Environments)
            Environments.Add(new EnvironmentViewModel(environment, DeleteEnvironment));
        _activeEnvironment = Environments.FirstOrDefault(e => e.Id == workspace.ActiveEnvironmentId) ?? _noEnvironment;
        _activeEnvironment.IsActive = true;
        EditableEnvironments = new ObservableCollection<EnvironmentViewModel>(Environments.Where(e => e != _noEnvironment));
        Environments.CollectionChanged += (_, e) =>
        {
            if (e.OldItems is not null)
                foreach (EnvironmentViewModel environment in e.OldItems)
                    EditableEnvironments.Remove(environment);
            if (e.NewItems is not null)
                foreach (EnvironmentViewModel environment in e.NewItems)
                    EditableEnvironments.Add(environment);

            // The page always shows an environment while there is one, including after a delete or an outside change.
            if (EditingEnvironment is null || !EditableEnvironments.Contains(EditingEnvironment))
                EditingEnvironment = EditableEnvironments.FirstOrDefault();
        };
        _selectedAuth = AuthOptions[0];

        // Last, because saving needs everything above. Moves data out of older formats,
        // including credentials from before Secrets existed.
        if (loaded.NeedsSave || WorkspaceSecrets.NeedsMigration(loaded.Workspace))
            _ = MigrateAsync();

        WatchFolder();
    }

    // Migrating leaves a backup behind, which should be offered for deletion straight away.
    private async Task MigrateAsync()
    {
        await PersistAsync();
        RefreshPlainTextCopies();
    }

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasPlainTextCopies))]
    private string _plainTextCopies = "";

    public bool HasPlainTextCopies => PlainTextCopies.Length > 0;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(DeleteCopiesLabel))]
    private bool _isConfirmingCopiesDelete;

    public string DeleteCopiesLabel => IsConfirmingCopiesDelete ? "Delete permanently?" : "Delete these copies";

    private void RefreshPlainTextCopies() => PlainTextCopies = string.Join('\n', _store.FindPlainTextCopies());

    [RelayCommand]
    private void DismissPlainTextCopies() => PlainTextCopies = "";

    // Deleting can't be undone, so it takes a second click within a few seconds.
    [RelayCommand(AllowConcurrentExecutions = true)]
    private async Task DeletePlainTextCopiesAsync()
    {
        if (!IsConfirmingCopiesDelete)
        {
            IsConfirmingCopiesDelete = true;
            await Task.Delay(3000);
            IsConfirmingCopiesDelete = false;
            return;
        }

        IsConfirmingCopiesDelete = false;
        var problems = await QueueStoreWork(() => _store.DeletePlainTextCopies());
        RefreshPlainTextCopies();
        if (problems.Count > 0)
            StorageWarning = JoinNotices(StorageWarning, $"Some copies could not be deleted:\n{string.Join('\n', problems)}");
    }

    public ObservableCollection<string> Methods { get; } = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

    public static IReadOnlyList<AuthOption> AuthOptions { get; } =
    [
        new(AuthKinds.None, "No auth"),
        new(AuthKinds.Bearer, "Bearer token"),
        new(AuthKinds.Basic, "Basic"),
        new(AuthKinds.ApiKey, "API key"),
    ];

    public ObservableCollection<TreeNodeViewModel> Collections { get; }
    public ObservableCollection<HistoryEntry> History { get; }
    public ObservableCollection<EnvironmentViewModel> Environments { get; }

    /// <summary>The environments without the "No environment" choice, which has nothing to edit.</summary>
    public ObservableCollection<EnvironmentViewModel> EditableEnvironments { get; }

    public KeyValueListViewModel Headers { get; } = new(keyPlaceholder: "Header name", valuePlaceholder: "Header value");
    public KeyValueListViewModel Globals { get; } = new(allowSecrets: true);

    [ObservableProperty] private string _requestName = "";
    [ObservableProperty] private string _method = "GET";
    [ObservableProperty] private string _url = "";

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(BodyTabTitle))]
    private string _body = "";

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(IsBearerAuth), nameof(IsBasicAuth), nameof(IsApiKeyAuth), nameof(IsNoAuth), nameof(AuthTabTitle))]
    private AuthOption _selectedAuth;

    public ObservableCollection<AssertionRowViewModel> Assertions { get; } = [];
    public ObservableCollection<TestResultRow> TestResults { get; } = [];

    public ObservableCollection<CaptureRowViewModel> Captures { get; } = [];

    /// <summary>The examples saved on the current Request (ADR 0023).</summary>
    public ObservableCollection<ResponseExampleRowViewModel> Examples { get; } = [];

    [ObservableProperty] private string _exampleName = "";

    /// <summary>True once there is a response that can be saved as an example.</summary>
    [ObservableProperty] private bool _canSaveExample;

    public bool HasExamples => Examples.Count > 0;
    public string ExamplesTabTitle => Examples.Count > 0 ? $"Examples  {Examples.Count}" : "Examples";

    /// <summary>Names only, because captures are often tokens. Their values live in memory, never in a file.</summary>
    public ObservableCollection<string> CapturedNames { get; } = [];

    private readonly Dictionary<string, List<KeyValue>> _capturedByCollection = [];

    private List<KeyValue> CapturedFor(string? collectionId) =>
        _capturedByCollection.TryGetValue(collectionId ?? "", out var values) ? values : [];

    private void RefreshCapturedNames()
    {
        CapturedNames.Clear();
        foreach (var value in CapturedFor(CurrentCollection?.Id))
            CapturedNames.Add(value.Key);
    }

    partial void OnCurrentCollectionChanged(TreeNodeViewModel? value) => RefreshCapturedNames();

    private void StoreCaptured(string? collectionId, IReadOnlyList<KeyValue> values)
    {
        if (!_capturedByCollection.TryGetValue(collectionId ?? "", out var stored))
            _capturedByCollection[collectionId ?? ""] = stored = [];

        foreach (var value in values)
        {
            stored.RemoveAll(v => v.Key == value.Key);
            stored.Add(value);
        }

        RefreshCapturedNames();
    }

    [RelayCommand]
    private void ClearCaptured()
    {
        _capturedByCollection.Remove(CurrentCollection?.Id ?? "");
        RefreshCapturedNames();
    }

    [RelayCommand]
    private void AddCapture() => Captures.Add(NewCaptureRow(new RequestCapture()));

    private CaptureRowViewModel NewCaptureRow(RequestCapture capture) => new(capture, row => Captures.Remove(row));

    public string TestsTabTitle => Assertions.Count + Captures.Count > 0 ? $"Tests  {Assertions.Count + Captures.Count}" : "Tests";
    public bool HasTestResults => TestResults.Count > 0;
    public string ResponseTestsTabTitle => TestResults.Count > 0
        ? $"Tests  {TestResults.Count(r => r.Passed)}/{TestResults.Count}"
        : "Tests";

    [RelayCommand]
    private void AddAssertion() => Assertions.Add(NewAssertionRow(new RequestAssertion { Expected = "200" }));

    private AssertionRowViewModel NewAssertionRow(RequestAssertion assertion) => new(assertion, row => Assertions.Remove(row));

    public string HeadersTabTitle => Headers.Items.Count > 0 ? $"Headers  {Headers.Items.Count}" : "Headers";
    public string BodyTabTitle => Body.Length > 0 ? "Body  •" : "Body";
    public string AuthTabTitle => IsNoAuth ? "Auth" : "Auth  •";

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasStorageWarning))]
    private string _storageWarning = "";

    public bool HasStorageWarning => StorageWarning.Length > 0;

    // Personal settings (ADR 0007). Each change is saved and applied at once, so there is no Save button.
    private PersonalSettings _settings = new();

    // Decimal, so that the numeric boxes can bind to them directly.
    public decimal RequestTimeoutSeconds
    {
        get => _settings.RequestTimeoutSeconds;
        set => ChangeSettings(s => s with { RequestTimeoutSeconds = (int)Math.Clamp(value, 1m, 3600m) }, nameof(RequestTimeoutSeconds));
    }

    public bool FollowRedirects
    {
        get => _settings.FollowRedirects;
        set => ChangeSettings(s => s with { FollowRedirects = value }, nameof(FollowRedirects));
    }

    public bool VerifyTlsCertificates
    {
        get => _settings.VerifyTlsCertificates;
        set => ChangeSettings(s => s with { VerifyTlsCertificates = value }, nameof(VerifyTlsCertificates));
    }

    public decimal MaxResponseMegabytes
    {
        get => _settings.MaxResponseMegabytes;
        set => ChangeSettings(s => s with { MaxResponseMegabytes = (int)Math.Clamp(value, 1m, 10240m) }, nameof(MaxResponseMegabytes));
    }

    public bool WrapResponse
    {
        get => _settings.WrapResponse;
        set => ChangeSettings(s => s with { WrapResponse = value }, nameof(WrapResponse));
    }

    /// <summary>Shown on every page while certificate checks are off, because that exposes credentials.</summary>
    public bool ShowTlsWarning => !_settings.VerifyTlsCertificates;

    private void ChangeSettings(Func<PersonalSettings, PersonalSettings> change, string propertyName)
    {
        _settings = change(_settings);
        _store.SaveSettings(_settings);
        ApplySettings();
        OnPropertyChanged(propertyName);
        OnPropertyChanged(nameof(ShowTlsWarning));
    }

    private void ApplySettings() => _sender.Configure(
        TimeSpan.FromSeconds(_settings.RequestTimeoutSeconds),
        _settings.FollowRedirects,
        _settings.VerifyTlsCertificates,
        _settings.MaxResponseMegabytes * 1024L * 1024L);

    [RelayCommand]
    private void DismissStorageWarning() => StorageWarning = "";

    public IReadOnlyList<string> ThemeChoices { get; } = ThemeSettings.Choices;

    /// <summary>System, Light or Dark. Kept on this machine only, in state.json.</summary>
    [ObservableProperty] private string _themeChoice = ThemeSettings.SystemChoice;

    partial void OnThemeChoiceChanged(string value) => _store.SetTheme(ThemeSettings.StoredValue(value));

    public string WorkspaceFolderPath => _store.FolderPath;
    public string WorkspaceFolderName => Path.GetFileName(_store.FolderPath.TrimEnd(Path.DirectorySeparatorChar));

    private void WatchFolder()
    {
        _folderWatcher?.Dispose();
        _folderWatcher = _store.Watch(() => Dispatcher.UIThread.Post(CheckForOutsideChanges));
        OnPropertyChanged(nameof(WorkspaceFolderPath));
        OnPropertyChanged(nameof(WorkspaceFolderName));
        RefreshRepositoryStatusCommand.Execute(null);
    }

    // The branch of the Workspace's git repository, read-only (ADR 0011). Refreshed when the folder changes and when the window is activated.
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasRepositoryStatus), nameof(RepositoryStatusText))]
    private GitStatus? _repositoryStatus;

    public bool HasRepositoryStatus => RepositoryStatus is not null;

    public string RepositoryStatusText => RepositoryStatus switch
    {
        null => "",
        { Ahead: null } status => status.Branch,
        var status => $"{status.Branch}  ↑{status.Ahead} ↓{status.Behind}",
    };

    public string RepositoryStatusTip =>
        "Compared with what git last fetched, so it can be out of date. Fetch and pull with your own git tools.";

    [RelayCommand]
    private async Task RefreshRepositoryStatusAsync() =>
        RepositoryStatus = await GitStatusReader.ReadAsync(_store.FolderPath);

    private void ShowIssues(IReadOnlyList<WorkspaceIssue> issues)
    {
        if (issues.Count == 0)
            return;

        var lines = issues.Select(i => $"{i.Path}: {i.Problem}");
        StorageWarning = JoinNotices(StorageWarning, $"Some workspace files could not be used, and were left untouched:\n{string.Join('\n', lines)}");
    }

    private Task<T> QueueStoreWork<T>(Func<T> work)
    {
        var task = _pendingSave.ContinueWith(_ => work(), TaskScheduler.Default);
        _pendingSave = task;
        return task;
    }

    // Files can change underneath the app, for example after a git pull.
    private async void CheckForOutsideChanges()
    {
        try
        {
            var reloaded = await QueueStoreWork(() => _store.HasOutsideChanges() ? _store.ReloadShared() : default);
            if (reloaded.Shared is null)
                return;

            var secrets = WorkspaceSecrets.ForWorkspace(_storedSecrets, _store.WorkspaceId);
            StorageWarning = "";
            ShowIssues(reloaded.Issues);
            ApplySharedWork(WorkspaceSecrets.Merge(reloaded.Shared, secrets));
            if (WorkspaceSecrets.NeedsMigration(reloaded.Shared with { History = [] }))
                await PersistAsync();
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            StorageWarning = JoinNotices(StorageWarning, $"Could not reload your workspace: {ex.Message}");
        }
    }

    /// <summary>Replaces collections, environments and globals, keeping the user's place and any unsaved edits.</summary>
    private void ApplySharedWork(Workspace shared)
    {
        var expanded = AllNodes(Collections).Where(n => n.IsExpanded).Select(n => n.Id).ToHashSet();
        var currentId = _currentNode?.Id;
        var hadUnsavedEdits = _currentNode?.Request is { } before && !WorkspaceSecrets.SameContent(before, CurrentRequest());
        var collectionId = CurrentCollection?.Id;
        var activeId = ActiveEnvironment.Id;
        var editingId = EditingEnvironment?.Id;

        ActiveEnvironment = _noEnvironment;
        foreach (var environment in Environments.Where(e => e != _noEnvironment).ToList())
            Environments.Remove(environment);
        foreach (var environment in shared.Environments)
            Environments.Add(new EnvironmentViewModel(environment, DeleteEnvironment));
        ActiveEnvironment = Environments.FirstOrDefault(e => e.Id == activeId) ?? _noEnvironment;
        EditingEnvironment = EditableEnvironments.FirstOrDefault(e => e.Id == editingId) ?? EditableEnvironments.FirstOrDefault();

        _currentNode = null;
        SelectedNode = null;
        Collections.Clear();
        foreach (var collection in shared.Collections)
            Collections.Add(TreeNodeViewModel.FromCollection(collection, DeleteNode, RenameNode, AddFolder));
        foreach (var node in AllNodes(Collections))
            node.IsExpanded = expanded.Contains(node.Id);

        Globals.Load(shared.Globals);
        CurrentCollection = Collections.FirstOrDefault(c => c.Id == collectionId);

        if (currentId is null)
            return;

        var current = AllNodes(Collections).FirstOrDefault(n => n.Id == currentId);
        if (current?.Request is not { } request)
        {
            Notice = "This request was removed outside Ricochet. Saving will create it again.";
            return;
        }

        _currentNode = current;
        SelectedNode = current;
        if (hadUnsavedEdits)
            Notice = "This request changed outside Ricochet. Your unsaved edits are still here, and saving will replace that version.";
        else if (!WorkspaceSecrets.SameContent(request, CurrentRequest()))
            LoadRequest(request);
    }

    private static IEnumerable<TreeNodeViewModel> AllNodes(IEnumerable<TreeNodeViewModel> nodes) =>
        nodes.SelectMany(n => AllNodes(n.Children).Prepend(n));

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasPendingCopy), nameof(PendingCopyMessage))]
    private string _pendingCopyFolder = "";

    public bool HasPendingCopy => PendingCopyFolder.Length > 0;
    public string PendingCopyMessage => $"{PendingCopyFolder} doesn't hold a workspace yet. Copy your current workspace there? Your files in that folder won't be changed, and the current workspace stays where it is.";

    [RelayCommand]
    private void CancelCopy() => PendingCopyFolder = "";

    [RelayCommand]
    private async Task ConfirmCopyAsync()
    {
        var path = PendingCopyFolder;
        PendingCopyFolder = "";
        await SwitchFolderAsync(path);
    }

    /// <summary>Opens the Workspace in a folder. A folder without one needs confirming, because it gets files added.</summary>
    public async Task OpenWorkspaceFolderAsync(string path)
    {
        if (WorkspaceFolder.LooksLikeWorkspace(path))
            await SwitchFolderAsync(path);
        else
            PendingCopyFolder = path;
    }

    private async Task SwitchFolderAsync(string path)
    {
        var (clean, _) = WorkspaceSecrets.Split(Snapshot());
        try
        {
            await PersistAsync();
            var (result, copied) = await QueueStoreWork(() => _store.OpenFolder(path, clean));
            WatchFolder();

            StorageWarning = "";
            ShowIssues(result.Issues);
            if (copied)
            {
                // The in-memory values are complete, and just need storing under the new Workspace's id.
                await PersistAsync();
                Notice = $"Your workspace was copied to {path}. The previous folder was left as it was.";
            }
            else
            {
                NewRequest();
                ApplySharedWork(WorkspaceSecrets.Merge(result.Workspace, WorkspaceSecrets.ForWorkspace(_storedSecrets, _store.WorkspaceId)));
            }
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            StorageWarning = JoinNotices(StorageWarning, $"Could not open that folder: {ex.Message}");
        }
    }

    private IReadOnlyDictionary<string, string> LoadSecrets()
    {
        try
        {
            return _vault.Load();
        }
        catch (SecretVaultException ex)
        {
            // Never write over a vault that could not be read. Secrets live in memory until the app closes.
            _vault = new InMemorySecretVault();
            StorageWarning = $"{ex.Message} Secrets you enter now will last only until you close Ricochet.";
            return new Dictionary<string, string>();
        }
    }

    [ObservableProperty] private string _authToken = "";
    [ObservableProperty] private string _authUsername = "";
    [ObservableProperty] private string _authPassword = "";
    [ObservableProperty] private string _authKeyName = "";
    [ObservableProperty] private string _authKeyValue = "";
    [ObservableProperty] private bool _authKeyInQuery;

    public bool IsNoAuth => SelectedAuth.Kind == AuthKinds.None;
    public bool IsBearerAuth => SelectedAuth.Kind == AuthKinds.Bearer;
    public bool IsBasicAuth => SelectedAuth.Kind == AuthKinds.Basic;
    public bool IsApiKeyAuth => SelectedAuth.Kind == AuthKinds.ApiKey;

    /// <summary>
    /// The value a {{name}} in the URL currently resolves to, for the hover tip. Null when it has no value
    /// or is a Secret, so a Secret's value is never shown on screen.
    /// </summary>
    public string? VariableValueForHover(string name)
    {
        IEnumerable<KeyValue>?[] scopes = [Globals.ToEntries(), CurrentCollection?.Variables.ToEntries(), FolderScope(), ActiveEnvironment.Variables.ToEntries()];
        var match = scopes.SelectMany(scope => scope ?? []).LastOrDefault(entry => entry.Enabled && entry.Key == name);
        if (match is null)
            return null;
        if (match.IsSecret)
            return "Secret, hidden";
        return match.Value;
    }

    /// <summary>The Variables of the Folders around the request on screen, outermost first (ADR 0020).</summary>
    private IReadOnlyList<KeyValue> FolderScope() =>
        _currentNode?.Root.ToCollection() is { } collection ? VariableResolver.FolderScope(collection, _currentNode.Id) : [];

    [ObservableProperty] private TreeNodeViewModel? _selectedNode;
    [ObservableProperty] private HistoryEntry? _selectedHistory;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasCurrentCollection), nameof(CollectionVariablesTitle))]
    private TreeNodeViewModel? _currentCollection;

    public bool HasCurrentCollection => CurrentCollection is not null;
    public string CollectionVariablesTitle => CurrentCollection is { } collection
        ? $"Variables of \"{collection.Name}\". These override Globals, and the active environment overrides these."
        : "Open a request from a collection to edit its variables.";

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasActiveEnvironment), nameof(IsEditingActive), nameof(UseEnvironmentLabel))]
    private EnvironmentViewModel _activeEnvironment;

    public bool HasActiveEnvironment => ActiveEnvironment != _noEnvironment;

    partial void OnActiveEnvironmentChanged(EnvironmentViewModel? oldValue, EnvironmentViewModel newValue)
    {
        if (oldValue is not null)
            oldValue.IsActive = false;
        if (newValue is not null)
            newValue.IsActive = true;
    }

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(IsSidePageOpen))]
    private bool _isEnvironmentsPageOpen;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasEditingEnvironment), nameof(IsEditingActive), nameof(UseEnvironmentLabel))]
    private EnvironmentViewModel? _editingEnvironment;

    public bool HasEditingEnvironment => EditingEnvironment is not null;
    public bool IsEditingActive => EditingEnvironment == ActiveEnvironment;
    public string UseEnvironmentLabel => IsEditingActive ? "Stop using" : "Use this environment";

    [RelayCommand]
    private void OpenEnvironmentsPage()
    {
        EditingEnvironment = HasActiveEnvironment ? ActiveEnvironment : EditingEnvironment ?? EditableEnvironments.FirstOrDefault();
        IsVariablesPageOpen = false;
        IsBulkPageOpen = false;
        IsSettingsPageOpen = false;
        IsEnvironmentsPageOpen = true;
    }

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(IsSidePageOpen))]
    private bool _isSettingsPageOpen;

    [RelayCommand]
    private void OpenSettingsPage()
    {
        IsEnvironmentsPageOpen = false;
        IsVariablesPageOpen = false;
        IsBulkPageOpen = false;
        IsSettingsPageOpen = true;
    }

    [RelayCommand]
    private void CloseSettingsPage() => IsSettingsPageOpen = false;

    /// <summary>
    /// Escape: closes the topmost thing that is open, using the same path as its own Cancel, Done or Close button.
    /// Dialogs come first, then a rename in the sidebar, then the side pages. Only one thing closes per press.
    /// </summary>
    [RelayCommand]
    private void CloseTopPanel()
    {
        if (HasPendingCopy)
            CancelCopy();
        else if (HasImportReport)
            CloseImportReport();
        else if (IsSpecDialogOpen)
            CloseSpecDialog();
        else if (AllNodes(Collections).FirstOrDefault(n => n.IsRenaming) is { } renaming)
            renaming.CancelRenameCommand.Execute(null);
        else if (IsBulkPageOpen)
            CloseBulkPage();
        else if (IsVariablesPageOpen)
            CloseVariablesPageCommand.Execute(null);
        else if (IsEnvironmentsPageOpen)
            CloseEnvironmentsPageCommand.Execute(null);
        else if (IsSettingsPageOpen)
            CloseSettingsPage();
    }

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(IsSidePageOpen))]
    private bool _isVariablesPageOpen;

    /// <summary>True while either page covers the request, so the request is hidden behind it.</summary>
    public bool IsSidePageOpen => IsEnvironmentsPageOpen || IsVariablesPageOpen || IsBulkPageOpen || IsSettingsPageOpen;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasEditingVariablesCollection))]
    private TreeNodeViewModel? _editingVariablesCollection;

    public bool HasEditingVariablesCollection => EditingVariablesCollection is not null;

    /// <summary>The collection or folder whose Variables the page edits. A folder's Variables apply to the requests inside it (ADR 0020).</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(VariablesScopeHint))]
    private TreeNodeViewModel? _editingVariablesScope;

    /// <summary>The collection being edited and each folder in it, for choosing what the page edits.</summary>
    public ObservableCollection<TreeNodeViewModel> VariableScopes { get; } = [];

    public string VariablesScopeHint => EditingVariablesScope is { Parent: not null }
        ? "Variables for the requests inside this folder. They override the collection's, and an environment's values override them."
        : "Variable names and defaults for this collection";

    // The Environments and Variables pages share one place on screen, so closing Environments also closes Variables.
    partial void OnIsEnvironmentsPageOpenChanged(bool value)
    {
        if (!value)
            IsVariablesPageOpen = IsBulkPageOpen = false;
    }

    /// <summary>The environment whose values the Variables page shows and edits.</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasVariablesEnvironment))]
    private EnvironmentViewModel? _variablesEnvironment;

    public bool HasVariablesEnvironment => VariablesEnvironment is not null;

    /// <summary>The collection's variables as the selected environment sees them, one row each.</summary>
    public ObservableCollection<VariableOverrideRowViewModel> VariableOverrides { get; } = [];

    partial void OnVariablesEnvironmentChanged(EnvironmentViewModel? value) => RebuildVariableOverrides();

    partial void OnEditingVariablesCollectionChanged(TreeNodeViewModel? oldValue, TreeNodeViewModel? newValue)
    {
        EditingVariablesScope = newValue;
        RefreshVariableScopes();
    }

    partial void OnEditingVariablesScopeChanged(TreeNodeViewModel? oldValue, TreeNodeViewModel? newValue)
    {
        // Clearing the picker's list also clears its selection, which must not leave the page editing nothing.
        if (newValue is null && EditingVariablesCollection is not null)
        {
            EditingVariablesScope = EditingVariablesCollection;
            return;
        }

        if (oldValue is not null)
            oldValue.Variables.Items.CollectionChanged -= OnVariableNamesChanged;
        if (newValue is not null)
            newValue.Variables.Items.CollectionChanged += OnVariableNamesChanged;
        RebuildVariableOverrides();
    }

    /// <summary>Lists the collection being edited, then every folder in it, in the order the sidebar shows them.</summary>
    private void RefreshVariableScopes()
    {
        var keep = EditingVariablesScope;
        VariableScopes.Clear();
        if (EditingVariablesCollection is not null)
            AddVariableScopes(EditingVariablesCollection);

        // A folder that is still listed stays selected. One that was deleted or moved away falls back to the collection.
        EditingVariablesScope = keep is not null && VariableScopes.Contains(keep) ? keep : EditingVariablesCollection;
    }

    private void AddVariableScopes(TreeNodeViewModel node)
    {
        VariableScopes.Add(node);
        foreach (var child in node.Children.Where(c => c.IsContainer))
            AddVariableScopes(child);
    }

    private void OnVariableNamesChanged(object? sender, NotifyCollectionChangedEventArgs e) => RebuildVariableOverrides();

    private void RebuildVariableOverrides()
    {
        VariableOverrides.Clear();
        if (EditingVariablesScope is null || VariablesEnvironment is null)
            return;

        foreach (var variable in EditingVariablesScope.Variables.Items.Where(row => row.Key.Trim().Length > 0))
            VariableOverrides.Add(new VariableOverrideRowViewModel(variable.Key, variable.Value, variable.IsSecret,
                VariablesEnvironment.Variables, VariablesEnvironment.Name, MoveVariable));
    }

    private async void MoveVariable(VariableOverrideRowViewModel row)
    {
        var defaults = EditingVariablesScope?.Variables;
        var environment = VariablesEnvironment;
        var variable = defaults?.Items.FirstOrDefault(r => r.Key == row.Key);
        if (defaults is null || environment is null || variable is null)
            return;

        // The environment keeps a value it already has. Otherwise it takes the collection's default.
        environment.Variables.AddIfMissing(variable.Key, variable.Value, variable.IsSecret);
        defaults.Items.Remove(variable);
        await PersistAsync();
    }

    [RelayCommand]
    /// <summary>Opens the Variables page for a collection, or for the current one when none is given.</summary>
    private void ShowVariablesPage(TreeNodeViewModel? collection)
    {
        IsEnvironmentsPageOpen = false;
        IsSettingsPageOpen = false;
        IsBulkPageOpen = false;
        EditingVariablesCollection = collection ?? CurrentCollection ?? EditingVariablesCollection ?? Collections.FirstOrDefault();
        EditingVariablesScope = EditingVariablesCollection;
        RefreshVariableScopes();
        VariablesEnvironment = HasActiveEnvironment ? ActiveEnvironment
            : EditableEnvironments.Contains(VariablesEnvironment!) ? VariablesEnvironment
            : EditableEnvironments.FirstOrDefault();
        IsVariablesPageOpen = true;
    }

    [RelayCommand]
    private async Task CloseVariablesPageAsync()
    {
        IsVariablesPageOpen = false;
        await PersistAsync();
    }

    [RelayCommand]
    private async Task CloseEnvironmentsPageAsync()
    {
        IsEnvironmentsPageOpen = false;
        IsSettingsPageOpen = false;
        await PersistAsync();
    }

    [RelayCommand]
    private void ToggleEditingActive()
    {
        if (EditingEnvironment is { } environment)
            ActiveEnvironment = IsEditingActive ? _noEnvironment : environment;
    }

    // The combo box clears its selection when the selected item is removed from its list.
    partial void OnActiveEnvironmentChanged(EnvironmentViewModel value)
    {
        if (value is null)
            ActiveEnvironment = _noEnvironment;
    }

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(SendButtonText), nameof(ShowEmptyState))]
    private bool _isSending;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasStatus), nameof(ShowEmptyState))]
    private string _statusText = "";

    public bool HasStatus => StatusText.Length > 0;
    public bool ShowEmptyState => !IsSending && !HasStatus;

    [ObservableProperty] private string _timingText = "";
    [ObservableProperty] private bool _isSuccess;
    [ObservableProperty] private bool _isFailure;
    [ObservableProperty] private string _responseHeaders = "";
    [ObservableProperty] private string _responseBody = "";

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasNotice))]
    private string _notice = "";

    public bool HasNotice => Notice.Length > 0;

    [RelayCommand]
    private void DismissNotice() => Notice = "";

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasImportReport))]
    private string _importReport = "";

    public bool HasImportReport => ImportReport.Length > 0;

    /// <summary>Whether <see cref="ResponseBody"/> is JSON. Set before the body so the view can pick highlighting.</summary>
    public bool ResponseIsJson { get; private set; }

    public string SendButtonText => IsSending ? "Cancel" : "Send";

    partial void OnSelectedNodeChanged(TreeNodeViewModel? value)
    {
        // Clicking a collection opens its variables, since a collection has no request to show.
        if (value is { Parent: null, Request: null })
        {
            ShowVariablesPage(value);
            return;
        }

        // Clicking a request brings it up over the Variables page, even when it is already the current one.
        if (value?.Request is not null)
            IsVariablesPageOpen = IsBulkPageOpen = false;

        // Matching the current node means it was just saved, so the editor and response are already right.
        if (value?.Request is not { } request || value == _currentNode)
            return;

        IsEnvironmentsPageOpen = false;
        IsSettingsPageOpen = false;
        SelectedHistory = null;
        _currentNode = value;
        CurrentCollection = value.Root;
        LoadRequest(request);
    }

    partial void OnSelectedHistoryChanged(HistoryEntry? value)
    {
        if (value is null)
            return;

        IsEnvironmentsPageOpen = false;
        IsSettingsPageOpen = false;
        SelectedNode = null;
        _currentNode = null; // Saving from history creates a new request.
        CurrentCollection = Collections.FirstOrDefault(c => c.Id == value.CollectionId);
        LoadRequest(value.Request);
        Notice = JoinNotices(Notice, CredentialNotice(value.Request));
    }

    private void LoadRequest(ApiRequest request)
    {
        if (!Methods.Contains(request.Method))
            Methods.Add(request.Method);

        RequestName = request.Name;
        Method = request.Method;
        Url = request.Url;
        Body = request.Body;
        Headers.Load(request.Headers);
        LoadAuth(request.Auth);
        Assertions.Clear();
        foreach (var assertion in request.Assertions ?? [])
            Assertions.Add(NewAssertionRow(assertion));
        Captures.Clear();
        foreach (var capture in request.Captures ?? [])
            Captures.Add(NewCaptureRow(capture));
        Examples.Clear();
        foreach (var example in request.Examples ?? [])
            Examples.Add(NewExampleRow(example));
        ClearResponse();
        Notice = ScriptNotice();
    }

    private void LoadAuth(RequestAuth? auth)
    {
        auth ??= new RequestAuth();
        SelectedAuth = AuthOptions.FirstOrDefault(o => o.Kind == auth.Kind) ?? AuthOptions[0];
        AuthToken = auth.Token;
        AuthUsername = auth.Username;
        AuthPassword = auth.Password;
        AuthKeyName = auth.KeyName;
        AuthKeyValue = auth.KeyValue;
        AuthKeyInQuery = auth.KeyInQuery;
    }

    private RequestAuth? CurrentAuth() => SelectedAuth.Kind == AuthKinds.None
        ? null
        : new RequestAuth
        {
            Kind = SelectedAuth.Kind,
            Token = AuthToken,
            Username = AuthUsername,
            Password = AuthPassword,
            KeyName = AuthKeyName,
            KeyValue = AuthKeyValue,
            KeyInQuery = AuthKeyInQuery,
        };

    private string ScriptNotice() => _currentNode?.HasScripts == true
        ? "This request has Postman scripts, on it or on a parent, which are not run. It may rely on values they would set."
        : "";

    private static string CredentialNotice(ApiRequest request)
    {
        var missingAuth = request.Auth switch
        {
            { Kind: AuthKinds.Bearer, Token: "" } => "a token",
            { Kind: AuthKinds.Basic, Password: "" } => "a password",
            { Kind: AuthKinds.ApiKey, KeyValue: "" } => "an API key value",
            _ => null,
        };

        return JoinNotices(
            missingAuth is null ? "" : $"Auth has no value for {missingAuth} on this machine.",
            WorkspaceSecrets.HasHiddenValues(request) ? $"Headers showing {SecretRules.HiddenValue} were removed from History, so enter them again." : "");
    }

    /// <summary>Says which lines of the .env file were skipped, so that a missing value isn't a mystery.</summary>
    internal static string DotEnvNotice(DotEnv.Result dotEnv) =>
        string.Join('\n', dotEnv.SkippedLines.Select(line => $"{DotEnv.FileName} line {line} was skipped, because it is not KEY=VALUE."));

    private static string JoinNotices(params string[] notices) =>
        string.Join('\n', notices.Where(n => n.Length > 0));

    private void ClearResponse()
    {
        StatusText = "";
        TimingText = "";
        IsSuccess = false;
        IsFailure = false;
        ResponseHeaders = "";
        ResponseIsJson = false;
        ResponseBody = "";
        Notice = "";
        TestResults.Clear();
        _lastExchange = null;
        CanSaveExample = false;
    }

    private ApiRequest CurrentRequest() => new()
    {
        Id = _currentNode?.Request?.Id ?? Guid.NewGuid().ToString("N"),
        Name = RequestName.Trim(),
        Method = Method,
        Url = Url,
        Body = Body,
        Headers = Headers.ToEntries(),
        Auth = CurrentAuth(),
        Assertions = Assertions.Count > 0 ? Assertions.Select(a => a.ToAssertion()).ToList() : null,
        Captures = Captures.Count > 0 ? Captures.Select(c => c.ToCapture()).ToList() : null,
        Examples = Examples.Count > 0 ? Examples.Select(e => e.Example).ToList() : null,
    };

    [RelayCommand]
    private void NewRequest()
    {
        IsEnvironmentsPageOpen = false;
        IsSettingsPageOpen = false;
        SelectedNode = null;
        SelectedHistory = null;
        _currentNode = null;
        CurrentCollection = null;
        LoadRequest(new ApiRequest());
    }

    [RelayCommand(AllowConcurrentExecutions = true)]
    private async Task SendOrCancelAsync()
    {
        if (_sendCts is not null)
        {
            _sendCts.Cancel();
            return;
        }

        // The request is hidden behind the page, so the shortcut must not send it unseen.
        if (IsEnvironmentsPageOpen || IsVariablesPageOpen || IsSettingsPageOpen)
            return;

        using var cts = new CancellationTokenSource();
        _sendCts = cts;
        IsSending = true;
        ClearResponse();

        var request = CurrentRequest();
        // History keeps this copy, with generated values filled in and Variables as written (ADR 0008).
        var historyRequest = request;
        var collectionId = CurrentCollection?.Id;
        var unresolved = "";
        var envProblems = "";
        try
        {
            // The .env file is read on each send, so an edit to it applies at once. Its values are overrides, as in the CLI (ADR 0021).
            var dotEnv = DotEnv.Read(_store.FolderPath);
            envProblems = DotEnvNotice(dotEnv);
            var variables = VariableResolver.Layer(
                Globals.ToEntries(),
                CurrentCollection?.Variables.ToEntries(),
                FolderScope(),
                ActiveEnvironment.Variables.ToEntries(),
                CapturedFor(collectionId),
                dotEnv.Variables);
            var resolution = VariableResolver.Resolve(request, variables);
            var resolved = resolution.Sent;
            historyRequest = resolution.History;
            var names = VariableResolver.FindPlaceholders(resolved);
            if (names.Count > 0)
                unresolved = $"No value for: {string.Join(", ", names)}";

            var response = await _sender.SendAsync(resolved, cts.Token);
            var formatted = await Task.Run(() => ResponseFormatter.Format(response), cts.Token);
            ShowResponse(response, formatted);
            _lastExchange = (resolved, variables, CapturedFor(collectionId), response);
            CanSaveExample = true;
            var testResults = await Task.Run(() => AssertionEvaluator.Evaluate(resolved.Assertions, response), cts.Token);
            foreach (var result in testResults)
            {
                var detail = result.Passed ? "" : $": {result.Message}";
                TestResults.Add(new TestResultRow(result.Passed ? "PASS" : "FAIL", result.Description + detail, result.Passed));
            }

            var captureOutcome = AssertionEvaluator.EvaluateCaptures(resolved.Captures, response);
            StoreCaptured(collectionId, captureOutcome.Values);
            foreach (var value in captureOutcome.Values)
                TestResults.Add(new TestResultRow("SET", value.Key, true));
            foreach (var failure in captureOutcome.Failures)
                TestResults.Add(new TestResultRow("FAIL", $"{failure.Description}: {failure.Message}", false));

            Notice = JoinNotices(ScriptNotice(), unresolved, envProblems, CredentialNotice(request), response.Truncated
                ? $"Response too large: showing the first {FormatSize(response.Body.Length)}."
                : "");
            AddHistory(historyRequest, response.StatusCode, (long)response.Elapsed.TotalMilliseconds, collectionId);
        }
        catch (OperationCanceledException)
        {
            StatusText = "Cancelled";
        }
        catch (RequestFailedException ex)
        {
            StatusText = "Failed";
            IsFailure = true;
            Notice = JoinNotices(ex.Message, unresolved, CredentialNotice(request), ScriptNotice());
            AddHistory(historyRequest, statusCode: null, elapsedMs: 0, collectionId);
        }
        finally
        {
            _sendCts = null;
            IsSending = false;
        }

        await PersistAsync();
    }

    private void ShowResponse(ApiResponse response, FormattedBody formatted)
    {
        StatusText = $"{response.StatusCode} {response.ReasonPhrase}".Trim();
        IsSuccess = response.StatusCode is >= 200 and < 400;
        IsFailure = !IsSuccess;
        TimingText = $"{response.Elapsed.TotalMilliseconds:N0} ms  ·  {FormatSize(response.Body.Length)}";
        ResponseHeaders = string.Join('\n', response.Headers.Select(h => $"{h.Key}: {h.Value}"));
        ResponseIsJson = formatted.IsJson;
        ResponseBody = formatted.Text;
    }

    private static string FormatSize(long bytes) => bytes switch
    {
        < 1024 => $"{bytes} B",
        < 1024 * 1024 => string.Create(CultureInfo.InvariantCulture, $"{bytes / 1024d:0.#} KB"),
        _ => string.Create(CultureInfo.InvariantCulture, $"{bytes / (1024d * 1024d):0.#} MB"),
    };

    private void AddHistory(ApiRequest request, int? statusCode, long elapsedMs, string? collectionId)
    {
        // Examples belong to the Request in the Workspace, not to each send of it.
        History.Insert(0, WorkspaceSecrets.SanitizeHistory(new HistoryEntry(request with { Examples = null }, statusCode, elapsedMs, DateTimeOffset.Now, collectionId)));
        while (History.Count > Workspace.MaxHistory)
            History.RemoveAt(History.Count - 1);
    }

    private ResponseExampleRowViewModel NewExampleRow(ResponseExample example) =>
        new(example, row => _ = RemoveExampleAsync(row), ShowExample);

    /// <summary>Saves the last response on the saved Request. Unsaved edits to the form stay unsaved.</summary>
    [RelayCommand]
    private async Task SaveExampleAsync()
    {
        if (_lastExchange is not { } last)
            return;

        if (_currentNode?.Request is not { } saved)
        {
            Notice = "Save the request first, then save a response as an example.";
            return;
        }

        var name = ExampleName.Trim().Length > 0 ? ExampleName : $"Example {Examples.Count + 1}";
        ResponseExample example;
        try
        {
            example = ResponseExamples.Create(name, last.Sent, last.Variables, last.Captured, last.Response);
        }
        catch (InvalidOperationException ex)
        {
            Notice = ex.Message;
            return;
        }

        _currentNode.Request = saved with { Examples = (saved.Examples ?? []).Append(example).ToList() };
        Examples.Add(NewExampleRow(example));
        ExampleName = "";
        Notice = $"Saved example \"{example.Name}\". Credentials were masked.";
        await PersistAsync();
    }

    private async Task RemoveExampleAsync(ResponseExampleRowViewModel row)
    {
        Examples.Remove(row);
        if (_currentNode?.Request is { } saved)
        {
            var remaining = (saved.Examples ?? []).Where(e => e.Id != row.Example.Id).ToList();
            _currentNode.Request = saved with { Examples = remaining.Count > 0 ? remaining : null };
        }

        await PersistAsync();
    }

    /// <summary>Shows a saved example in the response panel. It is not a new response, so it can't be saved again.</summary>
    private void ShowExample(ResponseExample example)
    {
        ClearResponse();
        var response = new ApiResponse(example.StatusCode, example.ReasonPhrase, example.Headers,
            System.Text.Encoding.UTF8.GetBytes(example.Body), example.MediaType, null, TimeSpan.Zero, false);
        ShowResponse(response, ResponseFormatter.Format(response));
        TimingText = $"Saved example \"{example.Name}\"";
        Notice = "This is a saved example, not a new response.";
    }

    [RelayCommand]
    private async Task SaveAsync()
    {
        // The request is hidden behind the page, so the shortcut saves what is on screen.
        if (IsEnvironmentsPageOpen || IsVariablesPageOpen || IsSettingsPageOpen)
        {
            await PersistAsync();
            return;
        }

        var request = CurrentRequest();
        if (request.Name.Length == 0)
            request = request with { Name = $"{request.Method} {request.Url}".Trim() };

        RequestName = request.Name;

        if (_currentNode is { } node)
        {
            node.Request = request;
            node.Name = request.Name;
        }
        else
        {
            var target = CurrentCollection ?? DefaultCollection();
            node = TreeNodeViewModel.ForNewRequest(request, target, DeleteNode, RenameNode, AddFolder);
            target.Children.Add(node);
            target.IsExpanded = true;
            _currentNode = node;
            CurrentCollection = target;
            SelectedHistory = null;
            SelectedNode = node;
        }

        var literals = SecretRules.FindLikelyLiterals(request);
        if (literals.Count > 0)
        {
            Notice = JoinNotices(ScriptNotice(),
                $"Saved as plain text: {string.Join(", ", literals)}. Use a {{{{variable}}}} marked Secret to keep it out of your files.");
        }

        await PersistAsync();
    }

    private TreeNodeViewModel DefaultCollection()
    {
        var existing = Collections.FirstOrDefault(c => c.Name == Workspace.DefaultCollectionName);
        if (existing is not null)
            return existing;

        var created = TreeNodeViewModel.FromCollection(new Collection { Name = Workspace.DefaultCollectionName }, DeleteNode, RenameNode, AddFolder);
        Collections.Add(created);
        return created;
    }

    /// <summary>The name is already changed on the node, so this only saves it.</summary>
    private async void RenameNode(TreeNodeViewModel node)
    {
        // A hidden item can't stay selected, since the tree no longer shows it.
        if (node.IsHidden && SelectedNode?.IsWithin(node) == true)
            SelectedNode = null;

        await PersistAsync();
    }

    /// <summary>Adds an empty folder inside the node, then opens its name for editing.</summary>
    private async void AddFolder(TreeNodeViewModel parent)
    {
        var name = WorkspaceImport.UniqueName("New folder", parent.Children.Select(c => c.Name));
        var folder = TreeNodeViewModel.ForNewFolder(name, parent, DeleteNode, RenameNode, AddFolder);
        parent.Children.Add(folder);
        parent.IsExpanded = true;
        SelectedNode = folder; // Highlights the new folder, which the tree then scrolls to.
        folder.StartRename();
        RefreshVariableScopes();
        await PersistAsync();
    }

    private async void DeleteNode(TreeNodeViewModel node)
    {
        if (EditingVariablesCollection?.IsWithin(node) == true)
            EditingVariablesCollection = null;

        if (_currentNode?.IsWithin(node) == true)
        {
            _currentNode = null;
            SelectedNode = null;
        }

        if (node.Parent is { } parent)
        {
            parent.Children.Remove(node);
        }
        else
        {
            Collections.Remove(node);
            if (CurrentCollection == node)
                CurrentCollection = null;
        }

        RefreshVariableScopes();
        await PersistAsync();
    }

    /// <summary>Moves a dragged node to where it was dropped, then saves. The caller checks that the drop is allowed.</summary>
    public async Task MoveNodeAsync(TreeNodeViewModel source, TreeNodeViewModel target, DropPlacement placement)
    {
        var destination = placement == DropPlacement.Into ? target : target.Parent;

        if (source.Parent is { } oldParent)
            oldParent.Children.Remove(source);
        else
            Collections.Remove(source);

        var siblings = destination?.Children ?? Collections;
        if (placement == DropPlacement.Into)
        {
            siblings.Add(source);
            target.IsExpanded = true;
        }
        else
        {
            var index = siblings.IndexOf(target) + (placement == DropPlacement.After ? 1 : 0);
            siblings.Insert(index, source);
        }

        source.MoveTo(destination);
        RefreshVariableScopes();
        await PersistAsync();
    }

    [RelayCommand]
    private async Task NewEnvironmentAsync()
    {
        var name = WorkspaceImport.UniqueName("New environment", Environments.Select(e => e.Name));
        var environment = new EnvironmentViewModel(new ApiEnvironment { Name = name }, DeleteEnvironment);
        Environments.Add(environment);
        ActiveEnvironment = environment;
        EditingEnvironment = environment;
        await PersistAsync();
    }

    private async void DeleteEnvironment(EnvironmentViewModel environment)
    {
        if (ActiveEnvironment == environment)
            ActiveEnvironment = _noEnvironment;

        Environments.Remove(environment);
        await PersistAsync();
    }

    /// <summary>The data dump whose exports the user is choosing from, or null. Shown over the window (ADR 0038).</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(IsDumpChoiceOpen))]
    private DumpChoiceViewModel? _dumpChoice;

    public bool IsDumpChoiceOpen => DumpChoice is not null;

    /// <summary>Imports each file on its own, so one bad file does not stop the others. A data dump opens a choice instead.</summary>
    public async Task ImportFilesAsync(IReadOnlyList<string> paths)
    {
        var dumps = paths.Where(IsDumpPath).ToList();
        var parsed = await Task.Run(() => paths.Where(p => !IsDumpPath(p)).Select(ParseFile).ToList());

        var report = new List<string>();
        foreach (var (name, result, error) in parsed)
            report.Add(result is null ? $"{name}\nNot imported. {error}" : $"{name}\n{ApplyImport(result)}");

        if (dumps.Count > 1)
            report.Add($"Import one data dump at a time. Not imported: {string.Join(", ", dumps.Skip(1).Select(Path.GetFileName))}");

        if (dumps.Count > 0)
        {
            var (name, listing, error) = await Task.Run(() => ReadDumpFile(dumps[0]));
            if (listing is null)
                report.Add($"{name}\nNot imported. {error}");
            else
                DumpChoice = new DumpChoiceViewModel(name, listing, ImportDumpAsync, CloseDumpChoice);
        }

        ImportReport = string.Join("\n\n", report);
        await PersistAsync();
    }

    private static bool IsDumpPath(string path) => Path.GetExtension(path).Equals(".zip", StringComparison.OrdinalIgnoreCase);

    private static (string Name, IReadOnlyList<DumpEntry>? Listing, string? Error) ReadDumpFile(string path)
    {
        var name = Path.GetFileName(path);
        try
        {
            using var stream = File.OpenRead(path);
            return (name, PostmanImporter.ReadDump(stream), null);
        }
        catch (Exception ex) when (ex is PostmanImportException or IOException or UnauthorizedAccessException)
        {
            return (name, null, ex.Message);
        }
    }

    /// <summary>Imports only the exports the user kept ticked. Nothing else in the dump is touched.</summary>
    private async Task ImportDumpAsync(DumpChoiceViewModel choice)
    {
        var results = PostmanImporter.Selected(choice.Listing, choice.SelectedFileNames);
        var report = results.Count == 0
            ? $"{choice.DumpName}\nNothing was selected, so nothing was imported."
            : $"{choice.DumpName}\n\n{string.Join("\n\n", results.Select(ApplyImport))}";

        DumpChoice = null;
        ImportReport = report;
        await PersistAsync();
    }

    [RelayCommand]
    private void CloseDumpChoice() => DumpChoice = null;

    private static (string Name, ImportResult? Result, string? Error) ParseFile(string path)
    {
        var name = Path.GetFileName(path);
        try
        {
            return (name, PostmanImporter.Import(File.ReadAllText(path), name), null);
        }
        catch (Exception ex) when (ex is PostmanImportException or IOException or UnauthorizedAccessException)
        {
            return (name, null, ex.Message);
        }
    }

    private string ApplyImport(ImportResult result)
    {
        var notes = new List<string>();

        if (result.Collection is { } collection)
        {
            var unique = WorkspaceImport.UniqueName(collection.Name, Collections.Select(c => c.Name));
            if (unique != collection.Name)
                notes.Add($"A collection with this name already exists, so it was added as \"{unique}\".");

            var node = TreeNodeViewModel.FromCollection(collection with { Name = unique }, DeleteNode, RenameNode, AddFolder);
            node.IsExpanded = true;
            Collections.Add(node);
        }

        if (result.Environment is { } environment)
        {
            var unique = WorkspaceImport.UniqueName(environment.Name, Environments.Select(e => e.Name));
            if (unique != environment.Name)
                notes.Add($"An environment with this name already exists, so it was added as \"{unique}\".");

            Environments.Add(new EnvironmentViewModel(environment with { Name = unique }, DeleteEnvironment));
            notes.Add("Select it from the environment menu to use it.");
        }

        if (result.Globals is { } globals)
        {
            var (merged, overwritten) = WorkspaceImport.MergeGlobals(Globals.ToEntries(), globals);
            Globals.Load(merged);
            if (overwritten.Count > 0)
                notes.Add($"Replaced existing values of: {string.Join(", ", overwritten)}");
        }

        var text = result.ToReportText().TrimEnd();
        return notes.Count > 0 ? $"{text}\n\n{string.Join('\n', notes)}" : text;
    }

    [RelayCommand]
    private void CloseImportReport() => ImportReport = "";

    [ObservableProperty] private bool _isSpecDialogOpen;
    [ObservableProperty] private string _specUrl = "";
    [ObservableProperty] private bool _isSpecBusy;
    [ObservableProperty] [NotifyPropertyChangedFor(nameof(HasSpecError))] private string _specError = "";
    [ObservableProperty] [NotifyPropertyChangedFor(nameof(HasSpecPreview))] private string _specPreview = "";

    public bool HasSpecError => SpecError.Length > 0;

    [RelayCommand]
    private void DismissSpecError() => SpecError = "";
    public bool HasSpecPreview => SpecPreview.Length > 0;

    private static readonly Lazy<HttpClient> SpecClient = new(() => RequestSender.CreateDefaultClient());
    private OpenApiResult? _pendingSpec;

    [RelayCommand]
    private void OpenSpecDialog()
    {
        SpecUrl = CurrentCollection?.SpecUrl ?? "";
        ClearSpecDialog();
        IsSpecDialogOpen = true;
    }

    /// <summary>Fetches only when the user presses Fetch, then shows what would change before anything is applied.</summary>
    [RelayCommand]
    private async Task FetchSpecAsync()
    {
        var address = SpecUrl.Trim();
        ClearSpecDialog();
        IsSpecBusy = true;
        try
        {
            var text = await OpenApiFetcher.FetchAsync(address, SpecClient.Value);
            var existing = Collections.FirstOrDefault(c => c.SpecUrl == address)?.ToCollection();
            var result = await Task.Run(() => OpenApiImporter.Build(text, address, existing));
            _pendingSpec = result;
            SpecPreview = result.Summary.ToReportText();
        }
        catch (OpenApiException ex)
        {
            SpecError = ex.Message;
        }
        finally
        {
            IsSpecBusy = false;
        }
    }

    [RelayCommand]
    private async Task ApplySpecAsync()
    {
        if (_pendingSpec is not { } result)
            return;

        var address = result.Collection.SpecUrl ?? "";
        if (result.Summary.IsNew)
        {
            var unique = WorkspaceImport.UniqueName(result.Collection.Name, Collections.Select(c => c.Name));
            var node = TreeNodeViewModel.FromCollection(result.Collection with { Name = unique }, DeleteNode, RenameNode, AddFolder);
            node.IsExpanded = true;
            Collections.Add(node);
        }
        else if (Collections.FirstOrDefault(c => c.SpecUrl == address) is { } old)
        {
            // The old node is detached, so the selection must not keep pointing at it.
            var node = TreeNodeViewModel.FromCollection(result.Collection, DeleteNode, RenameNode, AddFolder);
            node.IsExpanded = true;
            SelectedNode = null;
            Collections[Collections.IndexOf(old)] = node;
        }

        ImportReport = $"From {address}\n\n{result.Summary.ToReportText()}";
        IsSpecDialogOpen = false;
        ClearSpecDialog();
        await PersistAsync();
    }

    [RelayCommand]
    private void CloseSpecDialog()
    {
        IsSpecDialogOpen = false;
        ClearSpecDialog();
    }

    private void ClearSpecDialog()
    {
        _pendingSpec = null;
        SpecError = "";
        SpecPreview = "";
    }

    private Workspace Snapshot() => new()
    {
        Collections = Collections.Select(c => c.ToCollection()).ToList(),
        Environments = Environments.Where(e => e != _noEnvironment).Select(e => e.ToEnvironment()).ToList(),
        ActiveEnvironmentId = HasActiveEnvironment ? ActiveEnvironment.Id : null,
        Globals = Globals.ToEntries(),
        History = History.ToList(),
    };

    /// <summary>Saves run one after another so an older snapshot can never overwrite a newer one.</summary>
    private Task QueueSave()
    {
        var (clean, secrets) = WorkspaceSecrets.Split(Snapshot());
        _pendingSave = _pendingSave.ContinueWith(_ => Save(clean, secrets), TaskScheduler.Default);
        return _pendingSave;
    }

    // Secrets go first, so a crash in between can't leave a file whose values were never stored.
    // If the vault fails, the file is still saved without them: never fall back to plain text.
    private void Save(Workspace clean, Dictionary<string, string> secrets)
    {
        SecretVaultException? vaultFailure = null;
        var allSecrets = WorkspaceSecrets.WithWorkspace(_storedSecrets, _store.WorkspaceId, secrets);
        if (!SameSecrets(allSecrets, _storedSecrets))
        {
            try
            {
                _vault.Save(allSecrets);
                _storedSecrets = allSecrets;
            }
            catch (SecretVaultException ex)
            {
                vaultFailure = ex;
            }
        }

        _store.Save(clean);
        if (vaultFailure is not null)
            throw vaultFailure;
    }

    private static bool SameSecrets(IReadOnlyDictionary<string, string> a, IReadOnlyDictionary<string, string> b) =>
        a.Count == b.Count && a.All(pair => b.TryGetValue(pair.Key, out var value) && value == pair.Value);

    private async Task PersistAsync()
    {
        try
        {
            await QueueSave();
        }
        catch (SecretVaultException ex)
        {
            StorageWarning = $"{ex.Message} They will last only until you close Ricochet.";
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            Notice = $"Could not save your workspace: {ex.Message}";
        }
    }

    /// <summary>Blocks until everything is on disk. Called as the window closes.</summary>
    public void SaveBeforeExit()
    {
        _sendCts?.Cancel();
        try
        {
            QueueSave().Wait();
        }
        catch (AggregateException)
        {
            // The app is closing and there is nowhere left to show this.
        }
    }
}
