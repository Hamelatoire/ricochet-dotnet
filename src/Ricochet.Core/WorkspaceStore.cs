using System.Text.Json;
using System.Text.Json.Serialization;

namespace Ricochet.Core;

[JsonSourceGenerationOptions(
    WriteIndented = true,
    NewLine = "\n",
    PropertyNamingPolicy = JsonKnownNamingPolicy.CamelCase,
    DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull)]
[JsonSerializable(typeof(Workspace))]
[JsonSerializable(typeof(PersonalState))]
[JsonSerializable(typeof(List<HistoryEntry>))]
[JsonSerializable(typeof(List<KeyValue>))]
[JsonSerializable(typeof(BulkProgress))]
[JsonSerializable(typeof(PersonalSettings))]
[JsonSerializable(typeof(WindowPlacement))]
[JsonSerializable(typeof(WorkspaceMarkerFile))]
[JsonSerializable(typeof(CollectionFile))]
[JsonSerializable(typeof(FolderFile))]
[JsonSerializable(typeof(RequestFile))]
[JsonSerializable(typeof(ApiEnvironment))]
[JsonSerializable(typeof(EnvironmentFile))]
[JsonSerializable(typeof(ApiRequest))]
[JsonSerializable(typeof(Dictionary<string, string>))]
internal sealed partial class WorkspaceJsonContext : JsonSerializerContext;

/// <summary>Settings that belong to this person on this machine, and are never shared.</summary>
public sealed record PersonalState
{
    public string? WorkspaceFolder { get; set; }
    public string? ActiveEnvironmentId { get; set; }

    /// <summary>"light" or "dark"; null or missing follows the operating system.</summary>
    public string? Theme { get; set; }

    /// <summary>Where each bulk run stopped, so it can resume after the last id it processed.</summary>
    public IReadOnlyList<BulkProgress> Bulk { get; set; } = [];

    /// <summary>Preferences for how this machine sends requests and shows responses (ADR 0007).</summary>
    public PersonalSettings Settings { get; set; } = new();

    /// <summary>Where the main window was when the app last closed. Null means the default size and position.</summary>
    public WindowPlacement? Window { get; set; }
}

/// <summary>
/// Where the main window was. Position is in screen pixels; size is in device-independent units, as Avalonia uses.
/// Size and position are the window's normal (not maximised) bounds, so restoring it from maximised still works.
/// </summary>
public sealed record WindowPlacement
{
    public int X { get; set; }
    public int Y { get; set; }
    public double Width { get; set; }
    public double Height { get; set; }
    public bool IsMaximized { get; set; }

    /// <summary>False for values no real window could have, such as a saved file edited by hand. A method, so it isn't saved as a field.</summary>
    public bool IsReasonable() =>
        double.IsFinite(Width) && double.IsFinite(Height)
        && Width is >= 200 and <= 10000
        && Height is >= 150 and <= 10000
        && X is > -20000 and < 20000
        && Y is > -20000 and < 20000;
}

/// <summary>
/// Personal preferences. Missing values in state.json take these defaults, so a new option needs no migration.
/// The CLI does not read these; it has its own options.
/// </summary>
public sealed record PersonalSettings
{
    public int RequestTimeoutSeconds { get; set; } = 120;
    public bool FollowRedirects { get; set; } = true;

    /// <summary>When off, the app accepts any server certificate. The app warns while this is off.</summary>
    public bool VerifyTlsCertificates { get; set; } = true;
    public int MaxResponseMegabytes { get; set; } = 100;
    public bool WrapResponse { get; set; } = true;
}

/// <summary>The ids pasted for a Request's bulk run, and the last one that was processed.</summary>
public sealed record BulkProgress
{
    public string RequestId { get; set; } = "";
    public string Ids { get; set; } = "";
    public string? LastId { get; set; }
    public int Processed { get; set; }
}

public sealed record WorkspaceLoadResult(Workspace Workspace, IReadOnlyList<WorkspaceIssue> Issues, bool NeedsSave);

/// <summary>Reads the original single-file format, so it can be migrated.</summary>
public sealed class LegacyWorkspaceFile(string path)
{
    /// <summary>An unreadable file is kept beside the original rather than overwritten.</summary>
    public Workspace Load()
    {
        if (!File.Exists(path))
            return new Workspace();

        try
        {
            var workspace = JsonSerializer.Deserialize(File.ReadAllText(path), WorkspaceJsonContext.Default.Workspace);
            return workspace?.MigrateLegacyFields() ?? new Workspace();
        }
        catch (JsonException)
        {
            File.Move(path, $"{path}.corrupt-{DateTime.UtcNow:yyyyMMddHHmmss}", overwrite: true);
            return new Workspace();
        }
    }
}

/// <summary>
/// Keeps a Workspace across its two homes: a shareable folder, and personal files in app data.
/// See docs/adr/0003-git-friendly-workspace-files.md.
/// </summary>
public sealed class WorkspaceStore(string personalDirectory, string? oldAppCopy = null)
{
    private const string StateFileName = "state.json";
    private const string HistoryFileName = "history.json";
    private const string LegacyFileName = "workspace.json";
    private const string LegacyBackupSuffix = ".bak";
    private const string DefaultFolderName = "workspace";
    private const string OldAppFolderName = "PostmanLight";

    private readonly Lock _gate = new();
    private WorkspaceFolder? _folder;
    private bool _legacyFilePending;
    private PersonalState _state = new();

    public string FolderPath => Folder.Root;

    /// <summary>"light" or "dark", or null to follow the operating system. Personal, so never shared.</summary>
    public string? Theme => _state.Theme;

    public void SetTheme(string? theme)
    {
        lock (_gate)
            SaveState(_state with { Theme = theme });
    }

    /// <summary>This machine's preferences. Read them with the store lock, as the other personal state.</summary>
    public PersonalSettings Settings
    {
        get
        {
            lock (_gate)
                return _state.Settings;
        }
    }

    public void SaveSettings(PersonalSettings settings)
    {
        lock (_gate)
            SaveState(_state with { Settings = settings });
    }

    /// <summary>
    /// Where the main window was last time. Null when nothing is saved, or when the saved value is not one a window could have,
    /// so the window opens at its default size and position.
    /// </summary>
    public WindowPlacement? Window
    {
        get
        {
            lock (_gate)
                return _state.Window is { } window && window.IsReasonable() ? window : null;
        }
    }

    public void SaveWindow(WindowPlacement placement)
    {
        lock (_gate)
            SaveState(_state with { Window = placement });
    }

    /// <summary>The bulk run progress saved for each Request.</summary>
    public IReadOnlyList<BulkProgress> BulkProgresses
    {
        get
        {
            lock (_gate)
                return _state.Bulk;
        }
    }

    public void SetBulkProgress(BulkProgress progress)
    {
        lock (_gate)
            SaveState(_state with { Bulk = _state.Bulk.Where(p => p.RequestId != progress.RequestId).Append(progress).ToList() });
    }

    /// <summary>Adds a line to the bulk run log, which is kept on this machine only.</summary>
    public void AppendBulkLog(string line)
    {
        lock (_gate)
            File.AppendAllText(Path.Combine(personalDirectory, "bulk-log.txt"), $"{DateTime.Now:yyyy-MM-dd HH:mm:ss}  {line}{Environment.NewLine}");
    }

    /// <summary>Identifies the Workspace, so that each one keeps its own Secrets on this machine.</summary>
    public string WorkspaceId => Folder.Id;

    private WorkspaceFolder Folder => _folder ?? throw new InvalidOperationException("Load the store first.");

    public string SecretsPath => Path.Combine(personalDirectory, "secrets.dat");

    /// <summary>Where personal data lives. Brings data over from when the app had its previous name.</summary>
    public static string DefaultPersonalDirectory()
    {
        var overridePath = Environment.GetEnvironmentVariable("RICOCHET_DATA");
        if (!string.IsNullOrEmpty(overridePath))
            return overridePath;

        var appData = Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData);
        var directory = Path.Combine(appData, "Ricochet");
        if (!File.Exists(Path.Combine(directory, StateFileName)))
            AdoptFileFromOldName(Path.Combine(appData, OldAppFolderName, LegacyFileName), Path.Combine(directory, LegacyFileName));

        return directory;
    }

    /// <summary>Where Secrets live, for tools that only read them and shouldn't migrate anything.</summary>
    public static string DefaultSecretsPath()
    {
        var overridePath = Environment.GetEnvironmentVariable("RICOCHET_DATA");
        var directory = !string.IsNullOrEmpty(overridePath)
            ? overridePath
            : Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "Ricochet");
        return Path.Combine(directory, "secrets.dat");
    }

    /// <summary>Brings data over from when the app had its previous name. The original stays as a backup.</summary>
    public static void AdoptFileFromOldName(string oldPath, string currentPath)
    {
        if (File.Exists(currentPath) || !File.Exists(oldPath))
            return;

        Directory.CreateDirectory(Path.GetDirectoryName(Path.GetFullPath(currentPath))!);
        File.Copy(oldPath, currentPath);
    }

    /// <summary>
    /// Where the app's previous name kept its data, for the normal personal directory only.
    /// A custom one, such as a test's, must never reach outside itself.
    /// </summary>
    public static string? DefaultOldAppCopy()
    {
        if (!string.IsNullOrEmpty(Environment.GetEnvironmentVariable("RICOCHET_DATA")))
            return null;

        return Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), OldAppFolderName, LegacyFileName);
    }

    /// <summary>Older plain-text copies of the user's data that may still hold credentials.</summary>
    public IReadOnlyList<string> FindPlainTextCopies()
    {
        var candidates = new List<string> { Path.Combine(personalDirectory, LegacyFileName + LegacyBackupSuffix) };
        if (oldAppCopy is not null)
            candidates.Insert(0, oldAppCopy);

        return candidates.Where(File.Exists).ToList();
    }

    /// <summary>
    /// Permanently deletes the copies <see cref="FindPlainTextCopies"/> reports, and nothing else.
    /// Returns a problem for each one that could not be deleted.
    /// </summary>
    public IReadOnlyList<string> DeletePlainTextCopies()
    {
        lock (_gate)
        {
            var problems = new List<string>();
            foreach (var path in FindPlainTextCopies())
            {
                try
                {
                    File.Delete(path);

                    // The old app's folder held only this file, so don't leave it behind empty.
                    if (path == oldAppCopy && Path.GetDirectoryName(path) is { } folder && !Directory.EnumerateFileSystemEntries(folder).Any())
                        Directory.Delete(folder);
                }
                catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
                {
                    problems.Add($"{path}: {ex.Message}");
                }
            }

            return problems;
        }
    }

    public WorkspaceLoadResult Load()
    {
        lock (_gate)
        {
            var issues = new List<WorkspaceIssue>();
            var stateExists = File.Exists(Path.Combine(personalDirectory, StateFileName));
            var state = ReadPersonal(StateFileName, WorkspaceJsonContext.Default.PersonalState, issues) ?? new PersonalState();
            _state = state;
            var history = ReadPersonal(HistoryFileName, WorkspaceJsonContext.Default.ListHistoryEntry, issues) ?? [];

            var folderPath = state.WorkspaceFolder ?? DefaultFolderPath();
            if (state.WorkspaceFolder is not null && !Directory.Exists(folderPath))
            {
                // Possibly an unplugged drive, so report it rather than recreate an empty Workspace there.
                issues.Add(new WorkspaceIssue(folderPath, "This workspace folder was not found, so the default one is open instead."));
                folderPath = DefaultFolderPath();
            }

            _folder = new WorkspaceFolder(folderPath);
            var (shared, folderIssues) = _folder.Load();
            issues.AddRange(folderIssues);

            var legacyPath = Path.Combine(personalDirectory, LegacyFileName);
            if (!stateExists && File.Exists(legacyPath))
            {
                var legacy = new LegacyWorkspaceFile(legacyPath).Load();
                _legacyFilePending = File.Exists(legacyPath);
                return new WorkspaceLoadResult(legacy, issues, NeedsSave: true);
            }

            var workspace = shared with { ActiveEnvironmentId = state.ActiveEnvironmentId, History = history };
            return new WorkspaceLoadResult(workspace, issues, NeedsSave: !stateExists);
        }
    }

    /// <summary>
    /// Opens a folder that already holds a Workspace. Any other folder gets a copy of the current
    /// Workspace, with the original left where it was.
    /// </summary>
    public (WorkspaceLoadResult Result, bool Copied) OpenFolder(string path, Workspace current)
    {
        lock (_gate)
        {
            var folder = new WorkspaceFolder(Path.GetFullPath(path));
            var copy = !WorkspaceFolder.LooksLikeWorkspace(path);
            var (shared, issues) = folder.Load();
            if (copy)
            {
                folder.Save(current);
                shared = current;
            }

            _folder = folder;
            SaveState(_state with { WorkspaceFolder = RememberedFolder(), ActiveEnvironmentId = current.ActiveEnvironmentId });

            var workspace = shared with { ActiveEnvironmentId = current.ActiveEnvironmentId, History = current.History };
            return (new WorkspaceLoadResult(workspace, issues, NeedsSave: false), copy);
        }
    }

    public void Save(Workspace workspace)
    {
        lock (_gate)
        {
            Folder.Save(workspace);
            WritePersonal(HistoryFileName, workspace.History.ToList(), WorkspaceJsonContext.Default.ListHistoryEntry);
            SaveState(_state with { WorkspaceFolder = RememberedFolder(), ActiveEnvironmentId = workspace.ActiveEnvironmentId });

            if (_legacyFilePending)
            {
                var legacyPath = Path.Combine(personalDirectory, LegacyFileName);
                File.Move(legacyPath, legacyPath + LegacyBackupSuffix, overwrite: true);
                _legacyFilePending = false;
            }
        }
    }

    public bool HasOutsideChanges() => Folder.HasOutsideChanges();

    /// <summary>Rereads the shared files. History and the active Environment are personal, so they stay as they are.</summary>
    public (Workspace Shared, IReadOnlyList<WorkspaceIssue> Issues) ReloadShared()
    {
        lock (_gate)
        {
            var (shared, issues) = Folder.Load();
            return (shared, issues);
        }
    }

    public IDisposable Watch(Action changed) => Folder.Watch(changed);

    private string DefaultFolderPath() => Path.Combine(personalDirectory, DefaultFolderName);

    // The default folder is not recorded, so moving the personal directory keeps working.
    private string? RememberedFolder() =>
        Path.GetFullPath(Folder.Root).Equals(Path.GetFullPath(DefaultFolderPath()), StringComparison.OrdinalIgnoreCase) ? null : Folder.Root;

    private void SaveState(PersonalState state)
    {
        _state = state;
        WritePersonal(StateFileName, state, WorkspaceJsonContext.Default.PersonalState);
    }

    private T? ReadPersonal<T>(string fileName, System.Text.Json.Serialization.Metadata.JsonTypeInfo<T> type, List<WorkspaceIssue> issues) where T : class
    {
        var path = Path.Combine(personalDirectory, fileName);
        if (!File.Exists(path))
            return null;

        try
        {
            return JsonSerializer.Deserialize(File.ReadAllText(path), type);
        }
        catch (JsonException ex)
        {
            var kept = $"{path}.corrupt-{DateTime.UtcNow:yyyyMMddHHmmss}";
            File.Move(path, kept, overwrite: true);
            issues.Add(new WorkspaceIssue(kept, $"This file could not be read, so it was set aside: {ex.Message}"));
            return null;
        }
    }

    private void WritePersonal<T>(string fileName, T value, System.Text.Json.Serialization.Metadata.JsonTypeInfo<T> type)
    {
        Directory.CreateDirectory(personalDirectory);
        var path = Path.Combine(personalDirectory, fileName);
        var temp = path + ".tmp";
        File.WriteAllText(temp, JsonSerializer.Serialize(value, type));
        File.Move(temp, path, overwrite: true);
    }
}
