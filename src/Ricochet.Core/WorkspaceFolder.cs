using System.Text;
using System.Text.Json;
using System.Text.Json.Serialization.Metadata;

namespace Ricochet.Core;

/// <summary>A file that could not be used. It stays on disk untouched until someone fixes it.</summary>
public sealed record WorkspaceIssue(string Path, string Problem);

public sealed record WorkspaceMarkerFile
{
    public string Id { get; set; } = Guid.NewGuid().ToString("N");
    public int Format { get; set; } = 1;

    /// <summary>The Collection ids in the order the sidebar shows them.</summary>
    public IReadOnlyList<string> Order { get; set; } = [];
}

public sealed record CollectionFile
{
    public string Id { get; set; } = "";
    public string Name { get; set; } = "";
    public IReadOnlyList<KeyValue> Variables { get; set; } = [];
    public IReadOnlyList<StoredScript> Scripts { get; set; } = [];
    public IReadOnlyList<string> Order { get; set; } = [];
    public string? SpecUrl { get; set; }
    public IReadOnlyList<string>? HiddenItemIds { get; set; }
}

public sealed record FolderFile
{
    public string Id { get; set; } = "";
    public string Name { get; set; } = "";
    public IReadOnlyList<StoredScript> Scripts { get; set; } = [];
    public IReadOnlyList<string> Order { get; set; } = [];
}

public sealed record RequestFile
{
    public string Id { get; set; } = "";
    public string Name { get; set; } = "";
    public IReadOnlyList<StoredScript> Scripts { get; set; } = [];
    public ApiRequest? Request { get; set; }
}

/// <summary>
/// The shareable part of a Workspace, as one file per item. See docs/adr/0003-git-friendly-workspace-files.md.
/// Only files this class wrote or understood are ever changed or deleted, so a file that fails to parse
/// (for example, one with merge conflict markers) and files it does not recognise are always safe.
/// </summary>
public sealed class WorkspaceFolder(string root)
{
    public const string MarkerFileName = "ricochet.json";

    /// <summary>Written once, when a workspace folder is first saved, so personal files are not committed by accident (ADR 0031).</summary>
    public const string GitIgnoreFileName = ".gitignore";

    private static readonly string GitIgnoreContent = string.Join('\n',
    [
        "# Written by Ricochet when this workspace was created. Edit it as you like; Ricochet won't overwrite it.",
        "# Temporary files left by an interrupted save",
        "*.tmp",
        "# Personal and secret-holding files, in case they end up in this folder",
        "state.json",
        "history.json",
        "secrets.dat",
        ".env",
        "*.secret",
    ]) + '\n';
    private const string CollectionFileName = "collection.json";
    private const string FolderFileName = "folder.json";
    private const string GlobalsFileName = "globals.json";
    private const string CollectionsDir = "collections";
    private const string EnvironmentsDir = "environments";

    private readonly Lock _gate = new();

    // What each understood file means, in the form we would write it. A save writes only what differs.
    private readonly Dictionary<string, string> _understood = [];

    // Every file as last seen, including unreadable ones, so outside changes can be noticed.
    private Dictionary<string, string> _seen = [];

    // The file name each item already has. It only changes when the item is renamed in the app.
    private readonly Dictionary<string, (string Leaf, string Name)> _leaves = [];

    public string Root { get; } = root;
    public string Id { get; private set; } = "";

    public static bool LooksLikeWorkspace(string path) =>
        File.Exists(Path.Combine(path, MarkerFileName)) || Directory.Exists(Path.Combine(path, CollectionsDir));

    public (Workspace Workspace, List<WorkspaceIssue> Issues) Load()
    {
        lock (_gate)
        {
            _understood.Clear();
            _leaves.Clear();
            var issues = new List<WorkspaceIssue>();
            Directory.CreateDirectory(Root);
            _seen = Scan(issues);

            var marker = ReadFile(MarkerFileName, WorkspaceJsonContext.Default.WorkspaceMarkerFile, issues);
            Id = marker is { Id.Length: > 0 } ? marker.Id : Guid.NewGuid().ToString("N");

            var workspace = new Workspace
            {
                Collections = ReadCollections(marker?.Order ?? [], issues),
                Environments = ReadEnvironments(issues),
                Globals = ReadGlobals(issues),
            };
            return (workspace, issues);
        }
    }

    /// <summary>Writes what changed and removes what was deleted. Unknown and unreadable files are left alone.</summary>
    public void Save(Workspace workspace)
    {
        lock (_gate)
        {
            var isNew = !File.Exists(FullPath(MarkerFileName));
            var desired = Render(workspace);
            AddGitIgnore(desired, isNew);

            foreach (var (path, content) in desired)
            {
                if (_understood.TryGetValue(path, out var existing) && existing == content)
                    continue;

                WriteAtomically(path, content);
                _understood[path] = content;

                // The scan doesn't read .gitignore, so it is not an outside change when it is written.
                if (path != GitIgnoreFileName)
                    _seen[path] = content;
            }

            foreach (var path in _understood.Keys.Where(p => !desired.ContainsKey(p)).ToList())
            {
                File.Delete(FullPath(path));
                _understood.Remove(path);
                _seen.Remove(path);
                RemoveEmptyParents(path);
            }
        }
    }

    /// <summary>
    /// Keeps the .gitignore that Ricochet wrote, and writes one for a new workspace. A .gitignore the user already
    /// had is left alone, and once Ricochet has written one, the user's edits are kept.
    /// </summary>
    private void AddGitIgnore(Dictionary<string, string> desired, bool isNew)
    {
        if (_understood.TryGetValue(GitIgnoreFileName, out var written))
            desired[GitIgnoreFileName] = written;
        else if (isNew && !File.Exists(FullPath(GitIgnoreFileName)))
            desired[GitIgnoreFileName] = GitIgnoreContent;
    }

    /// <summary>True when files differ from what was last read or written, for example after a git pull.</summary>
    public bool HasOutsideChanges()
    {
        lock (_gate)
        {
            var current = Scan([]);
            return current.Count != _seen.Count
                || current.Any(pair => !_seen.TryGetValue(pair.Key, out var seen) || seen != pair.Value);
        }
    }

    public IDisposable Watch(Action changed) => new Watcher(Root, changed);

    private Dictionary<string, string> Scan(List<WorkspaceIssue> issues)
    {
        var found = new Dictionary<string, string>();

        void Add(string fullPath)
        {
            var relative = Path.GetRelativePath(Root, fullPath).Replace('\\', '/');
            try
            {
                found[relative] = File.ReadAllText(fullPath).Replace("\r\n", "\n");
            }
            catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
            {
                issues.Add(new WorkspaceIssue(relative, ex.Message));
            }
        }

        foreach (var name in new[] { MarkerFileName, GlobalsFileName })
        {
            var path = Path.Combine(Root, name);
            if (File.Exists(path))
                Add(path);
        }

        foreach (var dir in new[] { CollectionsDir, EnvironmentsDir })
        {
            var path = Path.Combine(Root, dir);
            if (Directory.Exists(path))
            {
                foreach (var file in Directory.EnumerateFiles(path, "*.json", SearchOption.AllDirectories))
                    Add(file);
            }
        }

        return found;
    }

    private List<Collection> ReadCollections(IReadOnlyList<string> order, List<WorkspaceIssue> issues)
    {
        var collections = new List<Collection>();
        var ids = new HashSet<string>();
        var dir = Path.Combine(Root, CollectionsDir);
        if (!Directory.Exists(dir))
            return collections;

        foreach (var collectionDir in Directory.EnumerateDirectories(dir).Order(StringComparer.Ordinal))
        {
            var relative = $"{CollectionsDir}/{Path.GetFileName(collectionDir)}";
            var file = ReadFile($"{relative}/{CollectionFileName}", WorkspaceJsonContext.Default.CollectionFile, issues);
            if (file is null || !HasUsableId(file.Id, $"{relative}/{CollectionFileName}", ids, issues))
                continue;

            _leaves[file.Id] = (Path.GetFileName(collectionDir), file.Name ?? "");
            collections.Add(new Collection
            {
                Id = file.Id,
                Name = file.Name ?? "",
                Variables = file.Variables ?? [],
                Scripts = file.Scripts ?? [],
                SpecUrl = file.SpecUrl,
                HiddenItemIds = file.HiddenItemIds,
                Items = ReadChildren(relative, file.Order ?? [], issues),
            });
        }

        // Follow the saved order. A Collection the order doesn't list goes last, by name, as a merge can add one.
        var rank = order.Select((id, index) => (id, index)).ToDictionary(p => p.id, p => p.index);
        return collections
            .OrderBy(c => rank.TryGetValue(c.Id, out var index) ? index : int.MaxValue)
            .ThenBy(c => c.Name, StringComparer.OrdinalIgnoreCase)
            .ToList();
    }

    private List<CollectionItem> ReadChildren(string relativeDir, IReadOnlyList<string> order, List<WorkspaceIssue> issues)
    {
        var fullDir = FullPath(relativeDir);
        var children = new Dictionary<string, CollectionItem>();
        var ids = new HashSet<string>();

        foreach (var filePath in Directory.EnumerateFiles(fullDir, "*.json").Order(StringComparer.Ordinal))
        {
            var name = Path.GetFileName(filePath);
            if (name is CollectionFileName or FolderFileName)
                continue;

            var relative = $"{relativeDir}/{name}";
            var file = ReadFile(relative, WorkspaceJsonContext.Default.RequestFile, issues);
            if (file is null || !HasUsableId(file.Id, relative, ids, issues))
                continue;

            if (file.Request is null)
            {
                Forget(relative);
                issues.Add(new WorkspaceIssue(relative, "It has no request."));
                continue;
            }

            _leaves[file.Id] = (Path.GetFileNameWithoutExtension(name), file.Name ?? "");
            children[file.Id] = new CollectionItem
            {
                Id = file.Id,
                Name = file.Name ?? "",
                Request = Normalize(file.Request),
                Scripts = file.Scripts ?? [],
            };
        }

        foreach (var childDir in Directory.EnumerateDirectories(fullDir).Order(StringComparer.Ordinal))
        {
            var relative = $"{relativeDir}/{Path.GetFileName(childDir)}";
            if (!File.Exists(FullPath($"{relative}/{FolderFileName}")))
            {
                if (Directory.EnumerateFiles(childDir, "*.json", SearchOption.AllDirectories).Any())
                    issues.Add(new WorkspaceIssue(relative, $"This folder has no {FolderFileName}, so its contents were not loaded."));
                continue;
            }

            var file = ReadFile($"{relative}/{FolderFileName}", WorkspaceJsonContext.Default.FolderFile, issues);
            if (file is null || !HasUsableId(file.Id, $"{relative}/{FolderFileName}", ids, issues))
                continue;

            _leaves[file.Id] = (Path.GetFileName(childDir), file.Name ?? "");
            children[file.Id] = new CollectionItem
            {
                Id = file.Id,
                Name = file.Name ?? "",
                Scripts = file.Scripts ?? [],
                Children = ReadChildren(relative, file.Order ?? [], issues),
            };
        }

        // Follow the saved order. A merge can add items it doesn't list, which then go last, by name.
        var result = new List<CollectionItem>();
        foreach (var id in order)
        {
            if (children.Remove(id, out var child))
                result.Add(child);
            else
                issues.Add(new WorkspaceIssue(relativeDir, $"The order lists an item that was not found: {id}"));
        }

        result.AddRange(children.Values.OrderBy(c => c.Name, StringComparer.OrdinalIgnoreCase));
        return result;
    }

    private bool HasUsableId(string? id, string relative, HashSet<string> ids, List<WorkspaceIssue> issues)
    {
        if (string.IsNullOrWhiteSpace(id))
        {
            Forget(relative);
            issues.Add(new WorkspaceIssue(relative, "It has no id."));
            return false;
        }

        if (!ids.Add(id))
        {
            Forget(relative);
            issues.Add(new WorkspaceIssue(relative, $"Another item already uses the id {id}."));
            return false;
        }

        return true;
    }

    // Anything we did not take into the model must never count as ours to delete.
    private void Forget(string relative) => _understood.Remove(relative);

    private static ApiRequest Normalize(ApiRequest request) => request with
    {
        Id = string.IsNullOrEmpty(request.Id) ? Guid.NewGuid().ToString("N") : request.Id,
        Name = request.Name ?? "",
        Method = string.IsNullOrEmpty(request.Method) ? "GET" : request.Method,
        Url = request.Url ?? "",
        Body = request.Body ?? "",
        Headers = request.Headers ?? [],
    };

    private List<ApiEnvironment> ReadEnvironments(List<WorkspaceIssue> issues)
    {
        var environments = new List<ApiEnvironment>();
        var ids = new HashSet<string>();
        var dir = Path.Combine(Root, EnvironmentsDir);
        if (!Directory.Exists(dir))
            return environments;

        foreach (var filePath in Directory.EnumerateFiles(dir, "*.json").Order(StringComparer.Ordinal))
        {
            var relative = $"{EnvironmentsDir}/{Path.GetFileName(filePath)}";
            var environment = ReadFile(relative, WorkspaceJsonContext.Default.ApiEnvironment, issues);
            if (environment is not null && HasUsableId(environment.Id, relative, ids, issues))
            {
                _leaves[environment.Id] = (Path.GetFileNameWithoutExtension(filePath), environment.Name ?? "");
                environments.Add(environment with { Name = environment.Name ?? "", Variables = environment.Variables ?? [] });
            }
        }

        return environments.OrderBy(e => e.Name, StringComparer.OrdinalIgnoreCase).ToList();
    }

    private List<KeyValue> ReadGlobals(List<WorkspaceIssue> issues) =>
        File.Exists(FullPath(GlobalsFileName))
            ? ReadFile(GlobalsFileName, WorkspaceJsonContext.Default.ListKeyValue, issues) ?? []
            : [];

    private T? ReadFile<T>(string relative, JsonTypeInfo<T> type, List<WorkspaceIssue> issues) where T : class
    {
        if (!_seen.TryGetValue(relative, out var text))
            return null;

        try
        {
            var value = JsonSerializer.Deserialize(text, type);
            if (value is null)
            {
                issues.Add(new WorkspaceIssue(relative, "The file is empty."));
                return null;
            }

            _understood[relative] = Serialize(value, type);
            return value;
        }
        catch (JsonException ex)
        {
            issues.Add(new WorkspaceIssue(relative, ex.Message));
            return null;
        }
    }

    private Dictionary<string, string> Render(Workspace workspace)
    {
        var files = new Dictionary<string, string>();

        void Add<T>(string path, T value, JsonTypeInfo<T> type) => files[path] = Serialize(value, type);

        bool UsedByAnotherItem(string candidate, bool isDirectory) => isDirectory
            ? files.Keys.Any(k => k.StartsWith(candidate + "/", StringComparison.Ordinal))
            : files.ContainsKey(candidate);

        // A new location must also avoid anything already on disk that we couldn't read.
        bool HoldsUnreadableFile(string candidate, bool isDirectory)
        {
            bool Blocks(string path) => !_understood.ContainsKey(path);

            return isDirectory
                ? _seen.Keys.Any(k => k.StartsWith(candidate + "/", StringComparison.Ordinal) && Blocks(k))
                : _seen.ContainsKey(candidate) && Blocks(candidate);
        }

        string PathFor(string parent, string name, string id, string extension)
        {
            var isDirectory = extension.Length == 0;
            var keepsLocation = _leaves.TryGetValue(id, out var known) && known.Name == name;
            var leaf = keepsLocation ? known.Leaf : Slug(name, id);
            var candidate = $"{parent}/{leaf}{extension}";

            if (UsedByAnotherItem(candidate, isDirectory) || (!keepsLocation && HoldsUnreadableFile(candidate, isDirectory)))
                leaf = $"{leaf}-{id}";

            _leaves[id] = (leaf, name);
            return $"{parent}/{leaf}{extension}";
        }

        void AddItem(CollectionItem item, string parent)
        {
            if (item.Request is not null)
            {
                Add(PathFor(parent, item.Name, item.Id, ".json"), new RequestFile { Id = item.Id, Name = item.Name, Scripts = item.Scripts, Request = item.Request },
                    WorkspaceJsonContext.Default.RequestFile);
                return;
            }

            var dir = PathFor(parent, item.Name, item.Id, "");
            Add($"{dir}/{FolderFileName}",
                new FolderFile { Id = item.Id, Name = item.Name, Scripts = item.Scripts, Order = item.Children.Select(c => c.Id).ToList() },
                WorkspaceJsonContext.Default.FolderFile);
            foreach (var child in item.Children)
                AddItem(child, dir);
        }

        Add(MarkerFileName, new WorkspaceMarkerFile { Id = Id, Order = workspace.Collections.Select(c => c.Id).ToList() }, WorkspaceJsonContext.Default.WorkspaceMarkerFile);

        foreach (var collection in workspace.Collections)
        {
            var dir = PathFor(CollectionsDir, collection.Name, collection.Id, "");
            Add($"{dir}/{CollectionFileName}", new CollectionFile
            {
                Id = collection.Id,
                Name = collection.Name,
                Variables = collection.Variables,
                Scripts = collection.Scripts,
                SpecUrl = collection.SpecUrl,
                HiddenItemIds = collection.HiddenItemIds,
                Order = collection.Items.Select(i => i.Id).ToList(),
            }, WorkspaceJsonContext.Default.CollectionFile);

            foreach (var item in collection.Items)
                AddItem(item, dir);
        }

        foreach (var environment in workspace.Environments)
            Add(PathFor(EnvironmentsDir, environment.Name, environment.Id, ".json"), environment, WorkspaceJsonContext.Default.ApiEnvironment);

        if (workspace.Globals.Count > 0 || _understood.ContainsKey(GlobalsFileName))
            Add(GlobalsFileName, workspace.Globals.ToList(), WorkspaceJsonContext.Default.ListKeyValue);

        return files;
    }

    /// <summary>The readable part of a file name, followed by part of the id so that equal names can't clash.</summary>
    public static string Slug(string name, string id)
    {
        const int maxNameLength = 40;
        const int idLength = 8;

        var slug = new StringBuilder();
        foreach (var c in name.ToLowerInvariant())
        {
            if (char.IsAsciiLetterOrDigit(c))
                slug.Append(c);
            else if (slug.Length > 0 && slug[^1] != '-')
                slug.Append('-');

            if (slug.Length >= maxNameLength)
                break;
        }

        var text = slug.ToString().Trim('-');
        var suffix = new string(id.Where(char.IsAsciiLetterOrDigit).Take(idLength).ToArray());
        return text.Length > 0 ? $"{text}-{suffix}" : $"item-{suffix}";
    }

    private static string Serialize<T>(T value, JsonTypeInfo<T> type) => JsonSerializer.Serialize(value, type) + "\n";

    private string FullPath(string relative) => Path.Combine(Root, relative.Replace('/', Path.DirectorySeparatorChar));

    private void WriteAtomically(string relative, string content)
    {
        var path = FullPath(relative);
        Directory.CreateDirectory(Path.GetDirectoryName(path)!);
        var temp = path + ".tmp";
        File.WriteAllText(temp, content);
        File.Move(temp, path, overwrite: true);
    }

    private void RemoveEmptyParents(string relative)
    {
        var dir = Path.GetDirectoryName(FullPath(relative));
        var rootFull = Path.GetFullPath(Root);
        while (dir is not null && !Path.GetFullPath(dir).Equals(rootFull, StringComparison.OrdinalIgnoreCase)
            && Directory.Exists(dir) && !Directory.EnumerateFileSystemEntries(dir).Any())
        {
            Directory.Delete(dir);
            dir = Path.GetDirectoryName(dir);
        }
    }

    private sealed class Watcher : IDisposable
    {
        private const int QuietPeriodMs = 500;

        private readonly FileSystemWatcher _watcher;
        private readonly Timer _timer;

        public Watcher(string root, Action changed)
        {
            _timer = new Timer(_ => changed());
            _watcher = new FileSystemWatcher(root)
            {
                IncludeSubdirectories = true,
                NotifyFilter = NotifyFilters.FileName | NotifyFilters.DirectoryName | NotifyFilters.LastWrite,
            };
            _watcher.Changed += OnEvent;
            _watcher.Created += OnEvent;
            _watcher.Deleted += OnEvent;
            _watcher.Renamed += OnEvent;
            _watcher.Error += (_, _) => Restart();
            _watcher.EnableRaisingEvents = true;
        }

        private void OnEvent(object sender, FileSystemEventArgs e)
        {
            var isGit = e.FullPath.Contains($"{Path.DirectorySeparatorChar}.git{Path.DirectorySeparatorChar}", StringComparison.Ordinal);
            if (!isGit)
                Restart();
        }

        // A git operation touches many files, so wait until things go quiet.
        private void Restart() => _timer.Change(QuietPeriodMs, Timeout.Infinite);

        public void Dispose()
        {
            _watcher.Dispose();
            _timer.Dispose();
        }
    }
}
