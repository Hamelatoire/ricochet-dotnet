using Ricochet.Core;

namespace Ricochet.Core.Tests;

public sealed class SidebarMenuTests
{
    [Fact]
    public void A_request_offers_hide_and_delete_but_not_rename_tags_or_new_folder()
    {
        Assert.False(SidebarMenu.Offers(SidebarAction.Rename, SidebarItemKind.Request, false, 0));
        Assert.False(SidebarMenu.Offers(SidebarAction.Tags, SidebarItemKind.Request, false, 0));
        Assert.False(SidebarMenu.Offers(SidebarAction.NewFolder, SidebarItemKind.Request, false, 0));
        Assert.True(SidebarMenu.Offers(SidebarAction.Hide, SidebarItemKind.Request, false, 0));
        Assert.True(SidebarMenu.Offers(SidebarAction.Delete, SidebarItemKind.Request, false, 0));
    }

    [Fact]
    public void A_folder_offers_rename_tags_new_folder_hide_and_delete()
    {
        foreach (var action in new[] { SidebarAction.Rename, SidebarAction.Tags, SidebarAction.NewFolder, SidebarAction.Hide, SidebarAction.Delete })
            Assert.True(SidebarMenu.Offers(action, SidebarItemKind.Folder, false, 0), action.ToString());
        Assert.False(SidebarMenu.Offers(SidebarAction.ShowHidden, SidebarItemKind.Folder, false, 3));
    }

    [Fact]
    public void A_collection_never_offers_hide_or_tags()
    {
        Assert.False(SidebarMenu.Offers(SidebarAction.Hide, SidebarItemKind.Collection, false, 0));
        Assert.False(SidebarMenu.Offers(SidebarAction.Tags, SidebarItemKind.Collection, false, 0));
        Assert.True(SidebarMenu.Offers(SidebarAction.Rename, SidebarItemKind.Collection, false, 0));
    }

    [Fact]
    public void Show_hidden_appears_on_a_collection_only_when_something_is_hidden()
    {
        Assert.False(SidebarMenu.Offers(SidebarAction.ShowHidden, SidebarItemKind.Collection, false, 0));
        Assert.True(SidebarMenu.Offers(SidebarAction.ShowHidden, SidebarItemKind.Collection, false, 2));
    }

    [Fact]
    public void Hide_label_says_show_for_a_hidden_item()
    {
        Assert.Equal("Hide", SidebarMenu.HideLabel(false));
        Assert.Equal("Show", SidebarMenu.HideLabel(true));
    }
}
