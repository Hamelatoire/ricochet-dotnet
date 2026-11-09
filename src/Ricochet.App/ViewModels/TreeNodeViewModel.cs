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

/// <summary>Where a dragged node lands, relative to the node it is dropped on.</summary>
public enum DropPlacement { Before, Into, After }

public sealed partial class TreeNodeViewModel : DeletableViewModel
{
    private readonly Action<TreeNodeViewModel> _delete;
    private readonly Action<TreeNodeViewModel> _rename;
    private readonly Action<TreeNodeViewModel> _addFolder;
    private readonly Action<TreeNodeViewModel> _duplicate;
    private readonly IReadOnlyList<StoredScript> _scripts;

    private TreeNodeViewModel(string id, TreeNodeViewModel? parent, IReadOnlyList<StoredScript> scripts,
        Action<TreeNodeViewModel> delete, Action<TreeNodeViewModel> rename, Action<TreeNodeViewModel> addFolder, Action<TreeNodeViewModel> duplicate)
    {
        Id = id;
        Parent = parent;
        _scripts = scripts;
        _delete = delete;
        _rename = rename;
        _addFolder = addFolder;
        _duplicate = duplicate;
        Children.CollectionChanged += (_, _) => SyncVisible();
    }

    public string Id { get; }
    public TreeNodeViewModel? Parent { get; private set; }
    public ObservableCollection<TreeNodeViewModel> Children { get; } = [];

    /// <summary>What the tree shows: Children without the hidden ones. Hidden items stay in Children, so they are saved (ADR 0032).</summary>
    public ObservableCollection<TreeNodeViewModel> VisibleChildren { get; } = [];

    [ObservableProperty] private bool _isHidden;

    /// <summary>On a collection: how many folders and requests inside it are hidden.</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HideLabel), nameof(ShowHiddenVisible))]
    private int _hiddenCount;

    /// <summary>The hide icon on a folder or request. Hidden by the row style until the row is hovered.</summary>
    public bool HideVisible => Parent is not null;

    /// <summary>The visible "n hidden" button on a collection, so hidden folders can be found again.</summary>
    public bool ShowHiddenVisible => Parent is null && HiddenCount > 0;

    /// <summary>The label of the button in the row: "Show n hidden" on a collection, a hide icon elsewhere.</summary>
    public string HideLabel => Parent is null ? (HiddenCount > 0 ? $"{HiddenCount} hidden" : "") : "⊘";

    private void SyncVisible()
    {
        VisibleChildren.Clear();
        var filter = Root._tagFilter;
        var inherited = TaggedWithAncestors(filter);
        foreach (var child in Children.Where(c => !c.IsHidden && c.MatchesTagFilter(filter, inherited)))
            VisibleChildren.Add(child);
    }

    // The tag filter lives on a collection and applies to all of its items (ADR 0030).
    private string _tagFilter = "";

    /// <summary>Shows only items with this tag, or inside a folder that has it, plus the folders that lead to them. Empty shows everything.</summary>
    public void ApplyTagFilter(string filter)
    {
        _tagFilter = filter;
        SyncVisibleTree();
    }

    private bool MatchesTagFilter(string filter, bool inherited)
    {
        if (filter.Length == 0)
            return true;

        var tagged = inherited || FolderTags.Contains(Tags, filter);
        return tagged || Children.Any(c => !c.IsHidden && c.MatchesTagFilter(filter, tagged));
    }

    private bool TaggedWithAncestors(string filter) =>
        filter.Length > 0 && (FolderTags.Contains(Tags, filter) || Parent?.TaggedWithAncestors(filter) == true);

    private void SyncVisibleTree()
    {
        SyncVisible();
        foreach (var child in Children)
            child.SyncVisibleTree();
    }

    private IEnumerable<TreeNodeViewModel> Descendants()
    {
        foreach (var child in Children)
        {
            yield return child;
            foreach (var descendant in child.Descendants())
                yield return descendant;
        }
    }

    private void RecountHidden() => HiddenCount = Descendants().Count(n => n.IsHidden);

    /// <summary>Hides a folder or request, or shows it again. On a collection, shows every hidden item (ADR 0032).</summary>
    [RelayCommand]
    private void HideOrShow()
    {
        if (Parent is null)
        {
            ShowAllHidden();
            return;
        }

        IsHidden = !IsHidden;
        Parent.SyncVisible();
        Root.RecountHidden();
        Root.OnHideChanged?.Invoke(this);
    }

    private void ShowAllHidden()
    {
        foreach (var node in Descendants().Where(n => n.IsHidden).ToList())
        {
            node.IsHidden = false;
            node.Parent?.SyncVisible();
        }

        RecountHidden();
        OnHideChanged?.Invoke(this);
    }

    /// <summary>Saves a change to hidden items. Set on a collection, and called by any node in it.</summary>
    public Action<TreeNodeViewModel>? OnHideChanged { get; set; }

    /// <summary>On a Collection or a Folder. A Folder's Variables apply to the Requests inside it (ADR 0020).</summary>
    public KeyValueListViewModel Variables { get; } = new(allowSecrets: true);

    /// <summary>The name shown when choosing which Collection or Folder to edit, such as "Shop / Users".</summary>
    public string ScopeLabel => Parent is null ? Name : $"{Parent.ScopeLabel} / {Name}";

    [ObservableProperty] private string _name = "";
    [ObservableProperty] private bool _isExpanded;

    /// <summary>True while the folder or collection name is being edited in the tree.</summary>
    [ObservableProperty] private bool _isRenaming;

    [ObservableProperty] private string _renameText = "";

    /// <summary>Tags on a folder (ADR 0030). Its requests inherit them. Empty on collections and requests.</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasTags), nameof(TagsLabel))]
    private IReadOnlyList<string> _tags = [];

    /// <summary>True while the folder's tags are being edited in the tree.</summary>
    [ObservableProperty] private bool _isEditingTags;

    /// <summary>The comma-separated tags being typed, shown while editing.</summary>
    [ObservableProperty] private string _tagsText = "";

    public bool HasTags => Tags.Count > 0;
    public string TagsLabel => string.Join(", ", Tags);

    /// <summary>Only folders take tags, so the tag button is shown on them alone.</summary>
    public bool TagsVisible => Parent is not null && Request is null;

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

    /// <summary>
    /// Where dropping this node onto <paramref name="target"/> would put it, or null if that is not allowed.
    /// <paramref name="fraction"/> is how far down the target's row the pointer is, from 0 (top) to 1 (bottom).
    /// Collections only reorder among Collections; other items can also go into a Folder or Collection.
    /// </summary>
    public DropPlacement? PlacementOnto(TreeNodeViewModel target, double fraction)
    {
        if (target.IsWithin(this))
            return null;

        var edge = fraction < 0.5 ? DropPlacement.Before : DropPlacement.After;

        if (Parent is null)
            return target.Parent is null ? edge : null;

        // Only a Folder or Collection can hold items, so it takes the middle of its row. Its edges still reorder.
        // The edges are wide enough that a folder can be dropped above or below another folder.
        if (target.IsContainer && fraction is > 0.3 and < 0.7)
            return DropPlacement.Into;

        // A Collection holds only Folders and Requests, so an edge drop on one can't reorder it.
        return target.Parent is null ? DropPlacement.Into : edge;
    }

    /// <summary>Puts this node in <paramref name="parent"/>'s children, or in the list of Collections when null.</summary>
    internal void MoveTo(TreeNodeViewModel? parent) => Parent = parent;

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

    /// <summary>Starts editing the tags, or saves them if they are already being edited.</summary>
    [RelayCommand]
    private void ToggleTags()
    {
        if (!TagsVisible)
            return;

        if (IsEditingTags)
        {
            CommitTags();
            return;
        }

        TagsText = string.Join(", ", Tags);
        IsEditingTags = true;
    }

    [RelayCommand]
    private void CommitTags()
    {
        IsEditingTags = false;
        var parsed = FolderTags.Parse(TagsText);
        if (parsed.SequenceEqual(Tags))
            return;

        Tags = parsed;
        Root.SyncVisibleTree();
        _rename(this); // Saved the same way a rename is.
    }

    [RelayCommand]
    private void CancelTags() => IsEditingTags = false;

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

    /// <summary>Makes a copy of this Request or Folder (ADR 0059). A Collection has nothing to copy it into.</summary>
    [RelayCommand]
    private void Duplicate()
    {
        if (Parent is not null)
            _duplicate(this);
    }

    /// <summary>Puts a copy made by <see cref="Ricochet.Core.ItemDuplicator"/> straight after this node, and returns its node.</summary>
    public TreeNodeViewModel AddCopy(CollectionItem copy, IReadOnlyCollection<string> hidden)
    {
        var parent = Parent!;
        var node = FromItem(copy, parent, _delete, _rename, _addFolder, _duplicate);
        foreach (var n in node.Descendants().Prepend(node))
            n.IsHidden = hidden.Contains(n.Id);

        parent.Children.Insert(parent.Children.IndexOf(this) + 1, node);
        parent.IsExpanded = true;
        Root.RecountHidden();
        return node;
    }

    public static TreeNodeViewModel FromCollection(Collection collection, Action<TreeNodeViewModel> delete, Action<TreeNodeViewModel> rename, Action<TreeNodeViewModel> addFolder, Action<TreeNodeViewModel> duplicate)
    {
        var node = new TreeNodeViewModel(collection.Id, null, collection.Scripts, delete, rename, addFolder, duplicate) { Name = collection.Name, SpecUrl = collection.SpecUrl };
        node.OnHideChanged = rename;
        node.Variables.Load(collection.Variables);
        foreach (var item in collection.Items)
            node.Children.Add(FromItem(item, node, delete, rename, addFolder, duplicate));

        var hidden = new HashSet<string>(collection.HiddenItemIds ?? []);
        foreach (var descendant in node.Descendants())
            descendant.IsHidden = hidden.Contains(descendant.Id);
        node.SyncVisibleTree();
        node.RecountHidden();
        return node;
    }

    private static TreeNodeViewModel FromItem(CollectionItem item, TreeNodeViewModel parent, Action<TreeNodeViewModel> delete, Action<TreeNodeViewModel> rename, Action<TreeNodeViewModel> addFolder, Action<TreeNodeViewModel> duplicate)
    {
        var node = new TreeNodeViewModel(item.Id, parent, item.Scripts, delete, rename, addFolder, duplicate) { Name = item.Name, Request = item.Request, Tags = item.Tags ?? [] };
        node.Variables.Load(item.Variables);
        foreach (var child in item.Children)
            node.Children.Add(FromItem(child, node, delete, rename, addFolder, duplicate));
        return node;
    }

    public static TreeNodeViewModel ForNewRequest(ApiRequest request, TreeNodeViewModel parent, Action<TreeNodeViewModel> delete, Action<TreeNodeViewModel> rename, Action<TreeNodeViewModel> addFolder, Action<TreeNodeViewModel> duplicate) =>
        new(Guid.NewGuid().ToString("N"), parent, [], delete, rename, addFolder, duplicate) { Name = request.Name, Request = request };

    public static TreeNodeViewModel ForNewFolder(string name, TreeNodeViewModel parent, Action<TreeNodeViewModel> delete, Action<TreeNodeViewModel> rename, Action<TreeNodeViewModel> addFolder, Action<TreeNodeViewModel> duplicate) =>
        new(Guid.NewGuid().ToString("N"), parent, [], delete, rename, addFolder, duplicate) { Name = name };

    public Collection ToCollection()
    {
        var hidden = Descendants().Where(n => n.IsHidden).Select(n => n.Id).ToList();
        return new()
        {
            Id = Id,
            Name = Name,
            Items = Children.Select(c => c.ToItem()).ToList(),
            Variables = Variables.ToEntries(),
            Scripts = _scripts,
            SpecUrl = SpecUrl,
            HiddenItemIds = hidden.Count > 0 ? hidden : null,
        };
    }

    private CollectionItem ToItem() => new()
    {
        Id = Id,
        Name = Name,
        Request = Request,
        Variables = Variables.ToEntries(),
        Children = Children.Select(c => c.ToItem()).ToList(),
        Scripts = _scripts,
        Tags = FolderTags.Normalize(Tags),
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
