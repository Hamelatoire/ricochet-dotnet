using Ricochet.Core;

namespace Ricochet.Core.Tests;

/// <summary>ADR 0059: Duplicate makes a full copy of a Request or Folder next to the original, with a unique name.</summary>
public class ItemDuplicatorTests
{
    private static CollectionItem Request(string id, string name, string url = "https://example.test") =>
        new() { Id = id, Name = name, Request = new ApiRequest { Id = id + "-req", Name = name, Url = url } };

    private static CollectionItem Folder(string id, string name, params CollectionItem[] children) =>
        new() { Id = id, Name = name, Children = children };

    [Fact]
    public void Copy_name_is_name_copy_then_numbered_when_taken()
    {
        Assert.Equal("Login (copy)", ItemDuplicator.CopyName("Login", ["Other"]));
        Assert.Equal("Login (copy 2)", ItemDuplicator.CopyName("Login", ["Login (copy)"]));
        Assert.Equal("Login (copy 3)", ItemDuplicator.CopyName("Login", ["Login (copy)", "login (copy 2)"]));
    }

    [Fact]
    public void Duplicating_a_request_puts_the_copy_straight_after_it_with_new_ids()
    {
        var collection = new Collection { Name = "Shop", Items = [Request("a", "Get"), Request("b", "Post")] };

        var result = ItemDuplicator.Duplicate(collection, "a")!;

        var names = result.Collection.Items.Select(i => i.Name).ToList();
        Assert.Equal(["Get", "Get (copy)", "Post"], names);

        var copy = result.Copy;
        Assert.Equal(copy, result.Collection.Items[1]);
        Assert.NotEqual("a", copy.Id);
        Assert.NotEqual("a-req", copy.Request!.Id);
        Assert.Equal("Get (copy)", copy.Request!.Name);
        Assert.Equal("https://example.test", copy.Request!.Url);
    }

    [Fact]
    public void The_copy_keeps_everything_that_belongs_to_the_request()
    {
        var original = Request("a", "Get") with
        {
            Request = new ApiRequest
            {
                Id = "a-req",
                Name = "Get",
                Url = "{{base}}/items",
                Headers = [new KeyValue("X-Trace", "1")],
                Body = "{}",
                Assertions = [new RequestAssertion { Path = "", Expected = "200" }],
                Captures = [new RequestCapture { Path = "id", Variable = "itemId" }],
                Auth = new RequestAuth { Kind = AuthKinds.Bearer, Token = "{{token}}" },
            },
            Tags = null,
        };
        var collection = new Collection { Name = "Shop", Items = [original] };

        var copy = ItemDuplicator.Duplicate(collection, "a")!.Copy.Request!;

        Assert.Equal("{{base}}/items", copy.Url);
        Assert.Equal("X-Trace", copy.Headers.Single().Key);
        Assert.Equal("{}", copy.Body);
        Assert.Equal("200", copy.Assertions!.Single().Expected);
        Assert.Equal("itemId", copy.Captures!.Single().Variable);
        Assert.Equal("{{token}}", copy.Auth!.Token);
    }

    [Fact]
    public void Duplicating_a_folder_copies_its_requests_with_new_ids_and_keeps_their_order()
    {
        var folder = Folder("f", "Smoke", Request("r1", "One"), Request("r2", "Two"));
        var collection = new Collection { Name = "Shop", Items = [folder] };

        var copy = ItemDuplicator.Duplicate(collection, "f")!.Copy;

        Assert.Equal("Smoke (copy)", copy.Name);
        Assert.Equal(["One", "Two"], copy.Children.Select(c => c.Name));
        Assert.DoesNotContain(copy.Children, c => c.Id is "r1" or "r2");
        Assert.Equal(2, copy.Children.Select(c => c.Id).Distinct().Count());
    }

    [Fact]
    public void Duplicating_a_nested_request_inserts_the_copy_in_its_own_folder()
    {
        var collection = new Collection
        {
            Name = "Shop",
            Items = [Folder("f", "Smoke", Request("r1", "One"), Request("r2", "Two")), Request("x", "Other")],
        };

        var result = ItemDuplicator.Duplicate(collection, "r1")!;

        var smoke = result.Collection.Items[0];
        Assert.Equal(["One", "One (copy)", "Two"], smoke.Children.Select(c => c.Name));
        Assert.Equal(["Smoke", "Other"], result.Collection.Items.Select(i => i.Name));
    }

    [Fact]
    public void Names_are_unique_among_siblings_only()
    {
        var collection = new Collection
        {
            Name = "Shop",
            Items = [Request("a", "Get"), Request("b", "Get (copy)"), Folder("f", "Box", Request("c", "Get"))],
        };

        var first = ItemDuplicator.Duplicate(collection, "a")!;
        Assert.Equal("Get (copy 2)", first.Copy.Name);

        var inside = ItemDuplicator.Duplicate(collection, "c")!;
        Assert.Equal("Get (copy)", inside.Copy.Name);
    }

    [Fact]
    public void A_copy_of_a_hidden_item_is_hidden_and_the_original_stays_hidden()
    {
        var collection = new Collection
        {
            Name = "Shop",
            Items = [Folder("f", "Smoke", Request("r1", "One"))],
            HiddenItemIds = ["f", "r1"],
        };

        var result = ItemDuplicator.Duplicate(collection, "f")!;

        Assert.Equal(2, result.Collection.HiddenItemIds!.Count(id => id == "f" || id == "r1"));
        var copyChild = result.Copy.Children.Single();
        Assert.Contains(copyChild.Id, result.Collection.HiddenItemIds!);
        Assert.Contains(result.Copy.Id, result.Collection.HiddenItemIds!);
    }

    [Fact]
    public void A_visible_item_copy_leaves_hidden_ids_unchanged()
    {
        var collection = new Collection { Name = "Shop", Items = [Request("a", "Get"), Request("b", "Post")], HiddenItemIds = ["b"] };

        var result = ItemDuplicator.Duplicate(collection, "a")!;

        Assert.Equal(["b"], result.Collection.HiddenItemIds);
    }

    [Fact]
    public void Unknown_id_returns_null_and_leaves_the_collection_alone()
    {
        var collection = new Collection { Name = "Shop", Items = [Request("a", "Get")] };

        Assert.Null(ItemDuplicator.Duplicate(collection, "missing"));
    }
}
