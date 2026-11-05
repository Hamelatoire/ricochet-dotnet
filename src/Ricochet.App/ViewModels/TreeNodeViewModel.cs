using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using Ricochet.Core;

namespace Ricochet.App.ViewModels;

/// <summary>A delete button that needs a second click within a few seconds, so a stray click loses nothing.</summary>
public abstract partial class DeletableViewModel : ObservableObject
{
    private const int ConfirmWindowMs = 3000;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(DeleteLabel))]
    private bool _isConfirmingDelete;

    public string DeleteLabel => IsConfirmingDelete ? "Delete?" : "✕";

    protected abstract void Delete();

    [RelayCommand(AllowConcurrentExecutions = true)]
    private async Task DeleteClickedAsync()
    {
        if (IsConfirmingDelete)
        {
            IsConfirmingDelete = false;
            Delete();
            return;
        }

        IsConfirmingDelete = true;
        await Task.Delay(ConfirmWindowMs);
        IsConfirmingDelete = false;
    }
}

public sealed partial class TreeNodeViewModel : DeletableViewModel
{
    private readonly Action<TreeNodeViewModel> _delete;
    private readonly Action<TreeNodeViewModel> _rename;
    private readonly Action<TreeNodeViewModel> _addFolder;
    private readonly IReadOnlyList<StoredScript> _scripts;

    private TreeNodeViewModel(string id, TreeNodeViewModel? parent, IReadOnlyList<StoredScript> scripts,
        Action<TreeNodeViewModel> delete, Action<TreeNodeViewModel> rename, Action<TreeNodeViewModel> addFolder)
    {
        Id = id;
        Parent = parent;
        _scripts = scripts;
        _delete = delete;
        _rename = rename;
        _addFolder = addFolder;
    }

    public string Id { get; }
    public TreeNodeViewModel? Parent { get; }
    public ObservableCollection<TreeNodeViewModel> Children { get; } = [];

    /// <summary>Only meaningful on a Collection node.</summary>
    public KeyValueListViewModel Variables { get; } = new(allowSecrets: true);

    [ObservableProperty] private string _name = "";
    [ObservableProperty] private bool _isExpanded;

    /// <summary>True while the folder or collection name is being edited in the tree.</summary>
    [ObservableProperty] private bool _isRenaming;

    [ObservableProperty] private string _renameText = "";

    /// <summary>Set on a Collection made from an OpenAPI URL (ADR 0014), so running the same URL updates it.</summary>
    public string? SpecUrl { get; set; }

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(MethodLabel), nameof(IsRequest), nameof(IsContainer))]
    private ApiRequest? _request;

    public string MethodLabel => Request?.Method ?? "";
    public bool IsRequest => Request is not null;
    public bool IsContainer => Request is null;

    public TreeNodeViewModel Root => Parent?.Root ?? this;

    public bool HasScripts => _scripts.Count > 0 || Parent?.HasScripts == true;

    public bool IsWithin(TreeNodeViewModel other) => this == other || Parent?.IsWithin(other) == true;

    protected override void Delete() => _delete(this);

    /// <summary>Starts editing the name, or saves it if it is already being edited.</summary>
    [RelayCommand]
    private void ToggleRename()
    {
        if (!IsContainer)
            return;

        if (IsRenaming)
        {
            CommitRename();
            return;
        }

        RenameText = Name;
        IsRenaming = true;
    }

    [RelayCommand]
    private void CommitRename()
    {
        var name = RenameText.Trim();
        IsRenaming = false;
        if (name.Length == 0 || name == Name)
            return;

        Name = name;
        _rename(this);
    }

    [RelayCommand]
    private void CancelRename() => IsRenaming = false;

    /// <summary>Opens the name for editing, as the rename button does.</summary>
    public void StartRename()
    {
        if (!IsContainer)
            return;

        RenameText = Name;
        IsRenaming = true;
    }

    /// <summary>Adds a folder inside this one. Only folders and collections can hold folders.</summary>
    [RelayCommand]
    private void NewFolder()
    {
        if (IsContainer)
            _addFolder(this);
    }

    public static TreeNodeViewModel FromCollection(Collection collection, Action<TreeNodeViewModel> delete, Action<TreeNodeViewModel> rename, Action<TreeNodeViewModel> addFolder)
    {
        var node = new TreeNodeViewModel(collection.Id, null, collection.Scripts, delete, rename, addFolder) { Name = collection.Name, SpecUrl = collection.SpecUrl };
        node.Variables.Load(collection.Variables);
        foreach (var item in collection.Items)
            node.Children.Add(FromItem(item, node, delete, rename, addFolder));
        return node;
    }

    private static TreeNodeViewModel FromItem(CollectionItem item, TreeNodeViewModel parent, Action<TreeNodeViewModel> delete, Action<TreeNodeViewModel> rename, Action<TreeNodeViewModel> addFolder)
    {
        var node = new TreeNodeViewModel(item.Id, parent, item.Scripts, delete, rename, addFolder) { Name = item.Name, Request = item.Request };
        foreach (var child in item.Children)
            node.Children.Add(FromItem(child, node, delete, rename, addFolder));
        return node;
    }

    public static TreeNodeViewModel ForNewRequest(ApiRequest request, TreeNodeViewModel parent, Action<TreeNodeViewModel> delete, Action<TreeNodeViewModel> rename, Action<TreeNodeViewModel> addFolder) =>
        new(Guid.NewGuid().ToString("N"), parent, [], delete, rename, addFolder) { Name = request.Name, Request = request };

    public static TreeNodeViewModel ForNewFolder(string name, TreeNodeViewModel parent, Action<TreeNodeViewModel> delete, Action<TreeNodeViewModel> rename, Action<TreeNodeViewModel> addFolder) =>
        new(Guid.NewGuid().ToString("N"), parent, [], delete, rename, addFolder) { Name = name };

    public Collection ToCollection() => new()
    {
        Id = Id,
        Name = Name,
        Items = Children.Select(c => c.ToItem()).ToList(),
        Variables = Variables.ToEntries(),
        Scripts = _scripts,
        SpecUrl = SpecUrl,
    };

    private CollectionItem ToItem() => new()
    {
        Id = Id,
        Name = Name,
        Request = Request,
        Children = Children.Select(c => c.ToItem()).ToList(),
        Scripts = _scripts,
    };
}

public sealed partial class EnvironmentViewModel : DeletableViewModel
{
    private readonly Action<EnvironmentViewModel> _delete;

    public EnvironmentViewModel(ApiEnvironment environment, Action<EnvironmentViewModel> delete)
    {
        Id = environment.Id;
        _name = environment.Name;
        _delete = delete;
        Variables.Load(environment.Variables);
    }

    public string Id { get; }
    public KeyValueListViewModel Variables { get; } = new(allowSecrets: true);

    [ObservableProperty] private string _name;
    [ObservableProperty] private bool _isActive;

    protected override void Delete() => _delete(this);

    public ApiEnvironment ToEnvironment() => new() { Id = Id, Name = Name, Variables = Variables.ToEntries() };
}
