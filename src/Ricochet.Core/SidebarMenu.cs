namespace Ricochet.Core;

/// <summary>What a sidebar item is: a Collection at the top, a Folder inside it, or a Request.</summary>
public enum SidebarItemKind { Collection, Folder, Request }

/// <summary>The actions in a sidebar item's context menu (ADR 0056). Each one is a command the item's buttons already use.</summary>
public enum SidebarAction { Rename, Tags, NewFolder, Hide, ShowHidden, Delete }

/// <summary>Which context menu entries apply to an item, so the menu never offers an action that would be refused.</summary>
public static class SidebarMenu
{
    public static bool Offers(SidebarAction action, SidebarItemKind kind, bool isHidden, int hiddenCount) => action switch
    {
        // Collections and Folders can be renamed and can take a new Folder. A Request keeps its name in the tree.
        SidebarAction.Rename or SidebarAction.NewFolder => kind != SidebarItemKind.Request,

        // Only Folders take tags (ADR 0030).
        SidebarAction.Tags => kind == SidebarItemKind.Folder,

        // A Collection is never hidden, so it has "Show hidden" instead of "Hide" when something inside is hidden.
        SidebarAction.Hide => kind != SidebarItemKind.Collection,
        SidebarAction.ShowHidden => kind == SidebarItemKind.Collection && hiddenCount > 0,

        SidebarAction.Delete => true,
        _ => false,
    };

    /// <summary>The label for hiding or showing an item: a hidden item offers "Show" (ADR 0032).</summary>
    public static string HideLabel(bool isHidden) => isHidden ? "Show" : "Hide";
}
