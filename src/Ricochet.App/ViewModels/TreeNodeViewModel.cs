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
    private readonly IReadOnlyList<StoredScript> _scripts;

    private TreeNodeViewModel(string id, TreeNodeViewModel? parent, IReadOnlyList<StoredScript> scripts, Action<TreeNodeViewModel> delete)
    {
        Id = id;
        Parent = parent;
        _scripts = scripts;
        _delete = delete;
    }

    public string Id { get; }
    public TreeNodeViewModel? Parent { get; }
    public ObservableCollection<TreeNodeViewModel> Children { get; } = [];

    /// <summary>Only meaningful on a Collection node.</summary>
    public KeyValueListViewModel Variables { get; } = new(allowSecrets: true);

    [ObservableProperty] private string _name = "";
    [ObservableProperty] private bool _isExpanded;

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

    public static TreeNodeViewModel FromCollection(Collection collection, Action<TreeNodeViewModel> delete)
    {
        var node = new TreeNodeViewModel(collection.Id, null, collection.Scripts, delete) { Name = collection.Name };
        node.Variables.Load(collection.Variables);
        foreach (var item in collection.Items)
            node.Children.Add(FromItem(item, node, delete));
        return node;
    }

    private static TreeNodeViewModel FromItem(CollectionItem item, TreeNodeViewModel parent, Action<TreeNodeViewModel> delete)
    {
        var node = new TreeNodeViewModel(item.Id, parent, item.Scripts, delete) { Name = item.Name, Request = item.Request };
        foreach (var child in item.Children)
            node.Children.Add(FromItem(child, node, delete));
        return node;
    }

    public static TreeNodeViewModel ForNewRequest(ApiRequest request, TreeNodeViewModel parent, Action<TreeNodeViewModel> delete) =>
        new(Guid.NewGuid().ToString("N"), parent, [], delete) { Name = request.Name, Request = request };

    public Collection ToCollection() => new()
    {
        Id = Id,
        Name = Name,
        Items = Children.Select(c => c.ToItem()).ToList(),
        Variables = Variables.ToEntries(),
        Scripts = _scripts,
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

    protected override void Delete() => _delete(this);

    public ApiEnvironment ToEnvironment() => new() { Id = Id, Name = Name, Variables = Variables.ToEntries() };
}
