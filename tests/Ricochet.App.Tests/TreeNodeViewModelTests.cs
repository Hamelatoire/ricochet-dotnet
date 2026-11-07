using Ricochet.Core;
using Ricochet.App.ViewModels;

namespace Ricochet.App.Tests;

public sealed class TreeNodeViewModelTests
{
    /// <summary>Records what the tree asks its owner to do, since the real owner saves the Workspace.</summary>
    private sealed class Owner
    {
        public List<TreeNodeViewModel> Renamed { get; } = [];
        public List<TreeNodeViewModel> AddedTo { get; } = [];
        public List<TreeNodeViewModel> Deleted { get; } = [];

        public TreeNodeViewModel Collection { get; }

        public Owner()
        {
            Collection = TreeNodeViewModel.FromCollection(Sample(), Deleted.Add, Renamed.Add, AddedTo.Add);
        }
    }

    private static Collection Sample() => new()
    {
        Id = "c1",
        Name = "Shop API",
        Items =
        [
            new CollectionItem
            {
                Id = "f1",
                Name = "Orders",
                Tags = ["vip"],
                Children =
                [
                    new CollectionItem { Id = "r1", Name = "Create order", Request = new ApiRequest { Id = "r1", Name = "Create order", Method = "POST", Url = "http://stub.test/orders" } },
                ],
            },
            new CollectionItem { Id = "r2", Name = "List products", Request = new ApiRequest { Id = "r2", Name = "List products", Url = "http://stub.test/products" } },
        ],
    };

    private static TreeNodeViewModel Child(TreeNodeViewModel parent, string name) => parent.Children.Single(c => c.Name == name);

    [Fact]
    public void Hiding_a_folder_removes_it_from_the_visible_rows_but_keeps_it_in_the_collection()
    {
        var owner = new Owner();
        var shop = owner.Collection;
        var orders = Child(shop, "Orders");

        orders.HideOrShowCommand.Execute(null);

        Assert.True(orders.IsHidden);
        Assert.DoesNotContain(orders, shop.VisibleChildren);
        Assert.Contains(orders, shop.Children);
        Assert.Equal(1, shop.HiddenCount);
        Assert.Equal("1 hidden", shop.HideLabel);
        Assert.True(shop.ShowHiddenVisible);
        Assert.Equal(new[] { orders }, owner.Renamed);
    }

    [Fact]
    public void Hidden_items_are_saved_as_hidden_ids_and_not_removed_from_items()
    {
        var owner = new Owner();
        Child(owner.Collection, "Orders").HideOrShowCommand.Execute(null);

        var saved = owner.Collection.ToCollection();

        Assert.Equal(2, saved.Items.Count);
        Assert.Equal(new[] { "f1" }, saved.HiddenItemIds);
    }

    [Fact]
    public void Hiding_a_request_inside_a_folder_is_counted_on_the_collection()
    {
        var owner = new Owner();
        var orders = Child(owner.Collection, "Orders");
        var create = Child(orders, "Create order");

        create.HideOrShowCommand.Execute(null);

        Assert.DoesNotContain(create, orders.VisibleChildren);
        Assert.Equal(1, owner.Collection.HiddenCount);
        Assert.Equal("⊘", create.HideLabel);
    }

    [Fact]
    public void Hiding_an_item_again_shows_it()
    {
        var owner = new Owner();
        var orders = Child(owner.Collection, "Orders");

        orders.HideOrShowCommand.Execute(null);
        orders.HideOrShowCommand.Execute(null);

        Assert.False(orders.IsHidden);
        Assert.Contains(orders, owner.Collection.VisibleChildren);
        Assert.Equal(0, owner.Collection.HiddenCount);
    }

    [Fact]
    public void Showing_from_the_collection_restores_every_hidden_item_inside_it()
    {
        var owner = new Owner();
        var orders = Child(owner.Collection, "Orders");
        var create = Child(orders, "Create order");
        orders.HideOrShowCommand.Execute(null);
        create.HideOrShowCommand.Execute(null);

        owner.Collection.HideOrShowCommand.Execute(null);

        Assert.False(orders.IsHidden);
        Assert.False(create.IsHidden);
        Assert.Equal(2, owner.Collection.VisibleChildren.Count);
        Assert.Equal(0, owner.Collection.HiddenCount);
        Assert.False(owner.Collection.ShowHiddenVisible);
        Assert.Null(owner.Collection.ToCollection().HiddenItemIds);
    }

    [Fact]
    public void Tag_filter_shows_tagged_folders_and_the_requests_inside_them_only()
    {
        var owner = new Owner();

        owner.Collection.ApplyTagFilter("vip");

        Assert.Equal(new[] { "Orders" }, owner.Collection.VisibleChildren.Select(c => c.Name));
        Assert.Equal(new[] { "Create order" }, Child(owner.Collection, "Orders").VisibleChildren.Select(c => c.Name));

        owner.Collection.ApplyTagFilter("");

        Assert.Equal(2, owner.Collection.VisibleChildren.Count);
    }

    [Fact]
    public void Renaming_a_folder_saves_the_trimmed_name()
    {
        var owner = new Owner();
        var orders = Child(owner.Collection, "Orders");

        orders.ToggleRenameCommand.Execute(null);
        Assert.True(orders.IsRenaming);
        Assert.Equal("Orders", orders.RenameText);

        orders.RenameText = "  Billing  ";
        orders.CommitRenameCommand.Execute(null);

        Assert.False(orders.IsRenaming);
        Assert.Equal("Billing", orders.Name);
        Assert.Equal(new[] { orders }, owner.Renamed);
    }

    [Fact]
    public void Renaming_to_the_same_or_an_empty_name_saves_nothing()
    {
        var owner = new Owner();
        var orders = Child(owner.Collection, "Orders");

        orders.ToggleRenameCommand.Execute(null);
        orders.RenameText = "Orders";
        orders.CommitRenameCommand.Execute(null);

        orders.ToggleRenameCommand.Execute(null);
        orders.RenameText = "   ";
        orders.CommitRenameCommand.Execute(null);

        Assert.Equal("Orders", orders.Name);
        Assert.Empty(owner.Renamed);
    }

    [Fact]
    public void A_request_cannot_be_renamed_in_the_tree()
    {
        var owner = new Owner();
        var list = Child(owner.Collection, "List products");

        list.ToggleRenameCommand.Execute(null);

        Assert.False(list.IsRenaming);
    }

    [Fact]
    public void New_folder_is_offered_on_folders_and_collections_but_not_requests()
    {
        var owner = new Owner();
        var orders = Child(owner.Collection, "Orders");

        owner.Collection.NewFolderCommand.Execute(null);
        orders.NewFolderCommand.Execute(null);
        Child(owner.Collection, "List products").NewFolderCommand.Execute(null);

        Assert.Equal(new[] { owner.Collection, orders }, owner.AddedTo);
    }
}
