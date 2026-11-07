using Ricochet.Core;

namespace Ricochet.Core.Tests;

public sealed class WorkspaceFolderTests : IDisposable
{
    private readonly string _root = Directory.CreateTempSubdirectory("ricochet-folder").FullName;

    public void Dispose() => Directory.Delete(_root, recursive: true);

    private static ApiRequest Request(string name, string id = "") => new()
    {
        Id = id.Length > 0 ? id : Guid.NewGuid().ToString("N"),
        Name = name,
        Url = $"http://x.test/{name}",
    };

    private static CollectionItem RequestItem(string name, string id) => new() { Id = id, Name = name, Request = Request(name, "r" + id) };

    private static Workspace Sample() => new()
    {
        Collections =
        [
            new Collection
            {
                Id = "c1aaaaaaaa",
                Name = "Shop API",
                Variables = [new("base", "http://x.test")],
                Scripts = [new StoredScript("test", "pm.a();")],
                Items =
                [
                    RequestItem("Zebra", "i1aaaaaaaa"),
                    new CollectionItem
                    {
                        Id = "f1aaaaaaaa",
                        Name = "Orders",
                        Scripts = [new StoredScript("prerequest", "pm.b();")],
                        Children = [RequestItem("Create", "i2aaaaaaaa")],
                    },
                    RequestItem("Apple", "i3aaaaaaaa"),
                ],
            },
        ],
        Environments = [new ApiEnvironment { Id = "e1aaaaaaaa", Name = "Staging", Variables = [new("a", "1")] }],
        Globals = [new("g", "1")],
    };

    private WorkspaceFolder Loaded()
    {
        var folder = new WorkspaceFolder(_root);
        folder.Load();
        return folder;
    }

    private WorkspaceFolder Saved(Workspace? workspace = null)
    {
        var folder = Loaded();
        folder.Save(workspace ?? Sample());
        return folder;
    }

    private string RequestPath(string name, string id) =>
        Path.Combine(_root, "collections", "shop-api-c1aaaaaa", $"{name}-{id}.json");

    private static Dictionary<string, DateTime> Times(string dir) =>
        Directory.EnumerateFiles(dir, "*", SearchOption.AllDirectories).ToDictionary(f => f, File.GetLastWriteTimeUtc);

    [Fact]
    public void Round_trips_everything_in_order()
    {
        Saved();

        var (loaded, issues) = new WorkspaceFolder(_root).Load();

        Assert.Empty(issues);
        var collection = Assert.Single(loaded.Collections);
        Assert.Equal("Shop API", collection.Name);
        Assert.Equal(["Zebra", "Orders", "Apple"], collection.Items.Select(i => i.Name));
        Assert.Equal("Create", collection.Items[1].Children[0].Name);
        Assert.Equal("ri2aaaaaaaa", collection.Items[1].Children[0].Request!.Id);
        Assert.Equal("pm.a();", collection.Scripts[0].Source);
        Assert.Equal("pm.b();", collection.Items[1].Scripts[0].Source);
        Assert.Equal("1", loaded.Environments[0].Variables[0].Value);
        Assert.Equal("1", loaded.Globals[0].Value);
    }

    [Fact]
    public void Collections_keep_the_order_they_were_saved_in()
    {
        var workspace = Sample();
        var second = new Collection { Id = "c2aaaaaaaa", Name = "Auth API" };
        var third = new Collection { Id = "c3aaaaaaaa", Name = "Billing" };
        workspace = workspace with { Collections = [third, workspace.Collections[0], second] };
        Saved(workspace);

        var (loaded, issues) = new WorkspaceFolder(_root).Load();

        Assert.Empty(issues);
        Assert.Equal(["Billing", "Shop API", "Auth API"], loaded.Collections.Select(c => c.Name));
    }

    [Fact]
    public void Uses_one_readable_file_per_request()
    {
        Saved();

        Assert.True(File.Exists(RequestPath("zebra", "i1aaaaaa")));
        Assert.True(File.Exists(Path.Combine(_root, "collections", "shop-api-c1aaaaaa", "orders-f1aaaaaa", "create-i2aaaaaa.json")));
        Assert.True(File.Exists(Path.Combine(_root, "environments", "staging-e1aaaaaa.json")));
        Assert.True(File.Exists(Path.Combine(_root, "globals.json")));
        Assert.True(File.Exists(Path.Combine(_root, WorkspaceFolder.MarkerFileName)));

        var text = File.ReadAllText(RequestPath("zebra", "i1aaaaaa"));
        Assert.DoesNotContain("\r\n", text);
        Assert.EndsWith("}\n", text);
    }

    [Fact]
    public void Id_stays_the_same_across_loads()
    {
        var first = Saved().Id;
        Assert.Equal(first, Loaded().Id);
        Assert.NotEmpty(first);
    }

    [Fact]
    public void Saving_writes_only_what_changed()
    {
        var folder = Saved();
        var before = Times(_root);
        Thread.Sleep(50);

        var sample = Sample();
        var changed = sample with
        {
            Collections =
            [
                sample.Collections[0] with
                {
                    Items = [sample.Collections[0].Items[0] with { Request = Request("Zebra", "ri1aaaaaaaa") with { Url = "http://changed.test" } }, sample.Collections[0].Items[1], sample.Collections[0].Items[2]],
                },
            ],
        };
        folder.Save(changed);

        var after = Times(_root);
        var written = after.Where(p => p.Value != before[p.Key]).Select(p => Path.GetFileName(p.Key)).ToList();
        Assert.Equal(["zebra-i1aaaaaa.json"], written);
    }

    [Fact]
    public void A_fresh_load_then_save_writes_nothing()
    {
        Saved();
        var before = Times(_root);
        Thread.Sleep(50);

        var folder = new WorkspaceFolder(_root);
        var (loaded, _) = folder.Load();
        folder.Save(loaded);

        Assert.Equal(before, Times(_root));
    }

    [Fact]
    public void Windows_line_endings_from_git_do_not_cause_rewrites()
    {
        Saved();
        var path = RequestPath("zebra", "i1aaaaaa");
        File.WriteAllText(path, File.ReadAllText(path).Replace("\n", "\r\n"));
        var before = File.GetLastWriteTimeUtc(path);
        Thread.Sleep(50);

        var folder = new WorkspaceFolder(_root);
        var (loaded, _) = folder.Load();
        folder.Save(loaded);

        Assert.Equal(before, File.GetLastWriteTimeUtc(path));
        Assert.False(folder.HasOutsideChanges());
    }

    [Fact]
    public void Deleting_and_renaming_remove_old_files_and_empty_folders()
    {
        var folder = Saved();
        var sample = Sample();

        folder.Save(sample with
        {
            Collections =
            [
                sample.Collections[0] with
                {
                    Name = "Renamed",
                    Items = [sample.Collections[0].Items[0]],
                },
            ],
            Environments = [],
        });

        Assert.False(Directory.Exists(Path.Combine(_root, "collections", "shop-api-c1aaaaaa")));
        Assert.True(File.Exists(Path.Combine(_root, "collections", "renamed-c1aaaaaa", "zebra-i1aaaaaa.json")));
        Assert.False(Directory.Exists(Path.Combine(_root, "environments")));
    }

    [Fact]
    public void Same_names_get_separate_files()
    {
        Saved(new Workspace
        {
            Collections =
            [
                new Collection { Id = "c1aaaaaaaa", Name = "Shop API", Items = [RequestItem("Same", "i1aaaaaaaa"), RequestItem("Same", "i2aaaaaaaa")] },
            ],
        });

        var (loaded, issues) = new WorkspaceFolder(_root).Load();
        Assert.Empty(issues);
        Assert.Equal(2, loaded.Collections[0].Items.Count);
    }

    [Fact]
    public void Ids_sharing_a_prefix_do_not_overwrite_each_other()
    {
        Saved(new Workspace
        {
            Collections =
            [
                new Collection { Id = "c1aaaaaaaa", Name = "Shop API", Items = [RequestItem("Same", "aaaaaaaa11"), RequestItem("Same", "aaaaaaaa22")] },
            ],
        });

        var (loaded, _) = new WorkspaceFolder(_root).Load();
        Assert.Equal(["aaaaaaaa11", "aaaaaaaa22"], loaded.Collections[0].Items.Select(i => i.Id).Order());
    }

    [Theory]
    [InlineData("Orders / Create #1!", "orders-create-1-abcdef12")]
    [InlineData("***", "item-abcdef12")]
    [InlineData("Écran", "cran-abcdef12")]
    public void Slugs_are_safe_for_every_file_system(string name, string expected)
    {
        Assert.Equal(expected, WorkspaceFolder.Slug(name, "abcdef1234567890"));
    }

    [Fact]
    public void Merge_conflict_is_reported_and_the_file_survives_saving()
    {
        Saved();
        var path = RequestPath("zebra", "i1aaaaaa");
        const string conflicted = "<<<<<<< HEAD\n{ }\n=======\n{ }\n>>>>>>> theirs\n";
        File.WriteAllText(path, conflicted);

        var folder = new WorkspaceFolder(_root);
        var (loaded, issues) = folder.Load();

        Assert.Contains(issues, i => i.Path.EndsWith("zebra-i1aaaaaa.json", StringComparison.Ordinal));
        Assert.DoesNotContain(loaded.Collections[0].Items, i => i.Name == "Zebra");
        Assert.Equal(["Orders", "Apple"], loaded.Collections[0].Items.Select(i => i.Name));

        folder.Save(loaded);
        Assert.Equal(conflicted, File.ReadAllText(path));
    }

    [Fact]
    public void Conflict_in_a_collection_file_keeps_all_of_its_children()
    {
        Saved();
        var collectionFile = Path.Combine(_root, "collections", "shop-api-c1aaaaaa", "collection.json");
        File.WriteAllText(collectionFile, "<<<<<<< broken");

        var folder = new WorkspaceFolder(_root);
        var (loaded, issues) = folder.Load();
        Assert.Empty(loaded.Collections);
        Assert.NotEmpty(issues);

        folder.Save(loaded);

        Assert.True(File.Exists(RequestPath("zebra", "i1aaaaaa")));
        Assert.Equal("<<<<<<< broken", File.ReadAllText(collectionFile));
    }

    [Fact]
    public void Item_a_merge_added_but_did_not_list_still_loads()
    {
        Saved();
        var extra = RequestPath("new-one", "i9aaaaaa");
        File.WriteAllText(extra, """{ "id": "i9aaaaaaaa", "name": "New One", "request": { "id": "x", "name": "New One", "url": "u" } }""");

        var (loaded, issues) = new WorkspaceFolder(_root).Load();

        Assert.Empty(issues);
        Assert.Equal(["Zebra", "Orders", "Apple", "New One"], loaded.Collections[0].Items.Select(i => i.Name));
    }

    [Fact]
    public void Order_naming_a_missing_file_is_reported()
    {
        Saved();
        File.Delete(RequestPath("zebra", "i1aaaaaa"));

        var (loaded, issues) = new WorkspaceFolder(_root).Load();

        Assert.Equal(["Orders", "Apple"], loaded.Collections[0].Items.Select(i => i.Name));
        Assert.Contains(issues, i => i.Problem.Contains("i1aaaaaaaa"));
    }

    [Fact]
    public void Copied_file_with_a_duplicate_id_is_reported_and_kept()
    {
        Saved();
        var copy = RequestPath("zebra-copy", "i1aaaaaa");
        File.Copy(RequestPath("zebra", "i1aaaaaa"), copy);

        var folder = new WorkspaceFolder(_root);
        var (loaded, issues) = folder.Load();
        Assert.Contains(issues, i => i.Problem.Contains("already uses"));

        folder.Save(loaded);
        Assert.True(File.Exists(copy));
        Assert.True(File.Exists(RequestPath("zebra", "i1aaaaaa")));
    }

    [Fact]
    public void A_duplicate_with_different_content_is_never_overwritten()
    {
        Saved();
        var original = RequestPath("zebra", "i1aaaaaa");
        var originalText = File.ReadAllText(original);
        var copy = RequestPath("zebra-copy", "i1aaaaaa");
        var copyText = originalText.Replace("http://x.test/Zebra", "http://edited.test");
        File.WriteAllText(copy, copyText);

        var folder = new WorkspaceFolder(_root);
        var (loaded, _) = folder.Load();
        folder.Save(loaded);

        Assert.Equal(originalText, File.ReadAllText(original).Replace("\r\n", "\n"));
        Assert.Equal(copyText, File.ReadAllText(copy).Replace("\r\n", "\n"));
    }

    [Fact]
    public void A_file_renamed_by_hand_keeps_its_name_until_the_item_is_renamed()
    {
        Saved();
        var renamed = RequestPath("my-own-name", "x");
        File.Move(RequestPath("zebra", "i1aaaaaa"), renamed);

        var folder = new WorkspaceFolder(_root);
        var (loaded, _) = folder.Load();
        folder.Save(loaded);
        Assert.True(File.Exists(renamed));
        Assert.False(File.Exists(RequestPath("zebra", "i1aaaaaa")));

        var collection = loaded.Collections[0];
        var items = collection.Items.Select(i => i.Id == "i1aaaaaaaa" ? i with { Name = "Giraffe" } : i).ToList();
        folder.Save(loaded with { Collections = [collection with { Items = items }] });
        Assert.False(File.Exists(renamed));
        Assert.True(File.Exists(RequestPath("giraffe", "i1aaaaaa")));
    }

    [Fact]
    public void A_new_item_never_writes_over_an_unreadable_file_at_its_path()
    {
        Saved();
        var blocked = RequestPath("apple", "i3aaaaaa");
        const string conflicted = "<<<<<<< conflict";
        File.WriteAllText(blocked, conflicted);

        var folder = new WorkspaceFolder(_root);
        var (loaded, _) = folder.Load();
        var collection = loaded.Collections[0];
        var restored = new CollectionItem { Id = "i3aaaaaaaa", Name = "Apple", Request = new ApiRequest { Name = "Apple", Url = "u" } };
        folder.Save(loaded with { Collections = [collection with { Items = [.. collection.Items, restored] }] });

        Assert.Equal(conflicted, File.ReadAllText(blocked));
        Assert.True(File.Exists(RequestPath("apple-i3aaaaaa", "i3aaaaaaaa")));
    }

    [Fact]
    public void Unknown_files_are_left_alone()
    {
        var folder = Saved();
        var readme = Path.Combine(_root, "README.md");
        var gitDir = Path.Combine(_root, ".git");
        Directory.CreateDirectory(gitDir);
        File.WriteAllText(readme, "hi");
        File.WriteAllText(Path.Combine(gitDir, "config"), "x");

        folder.Save(new Workspace());

        Assert.True(File.Exists(readme));
        Assert.True(File.Exists(Path.Combine(gitDir, "config")));
        Assert.False(folder.HasOutsideChanges());
    }

    [Fact]
    public void Explicit_nulls_in_a_hand_edited_file_do_not_crash()
    {
        Saved();
        File.WriteAllText(RequestPath("zebra", "i1aaaaaa"),
            """{ "id": "i1aaaaaaaa", "name": null, "scripts": null, "request": { "id": null, "name": null, "url": null, "method": null, "headers": null, "body": null } }""");

        var (loaded, _) = new WorkspaceFolder(_root).Load();

        var request = loaded.Collections[0].Items.Single(i => i.Id == "i1aaaaaaaa").Request!;
        Assert.Equal("GET", request.Method);
        Assert.Empty(request.Headers);
        Assert.Equal("", request.Url);
    }

    [Fact]
    public void Own_saves_are_not_outside_changes_but_other_edits_are()
    {
        var folder = Saved();
        Assert.False(folder.HasOutsideChanges());

        folder.Save(Sample() with { Globals = [new("g", "2")] });
        Assert.False(folder.HasOutsideChanges());

        File.WriteAllText(Path.Combine(_root, "globals.json"), """[ { "key": "g", "value": "3" } ]""");
        Assert.True(folder.HasOutsideChanges());

        var (loaded, _) = folder.Load();
        Assert.Equal("3", loaded.Globals[0].Value);
        Assert.False(folder.HasOutsideChanges());
    }

    [Fact]
    public void Added_and_removed_files_are_outside_changes()
    {
        var folder = Saved();
        var path = RequestPath("zebra", "i1aaaaaa");
        var text = File.ReadAllText(path);

        File.Delete(path);
        Assert.True(folder.HasOutsideChanges());

        File.WriteAllText(path, text);
        Assert.False(folder.HasOutsideChanges());
    }

    [Fact]
    public async Task Watcher_reports_a_change_once_things_go_quiet()
    {
        Saved();
        var folder = Loaded();
        var changed = new TaskCompletionSource();
        using var watcher = folder.Watch(() => changed.TrySetResult());

        File.WriteAllText(Path.Combine(_root, "globals.json"), "[]");

        var finished = await Task.WhenAny(changed.Task, Task.Delay(TimeSpan.FromSeconds(10), TestContext.Current.CancellationToken));
        Assert.Same(changed.Task, finished);
    }
}

public sealed class WorkspaceStoreTests : IDisposable
{
    private readonly string _dir = Directory.CreateTempSubdirectory("ricochet-store").FullName;

    public void Dispose() => Directory.Delete(_dir, recursive: true);

    private static Workspace Sample(string activeId = "e1aaaaaaaa") => new()
    {
        Collections = [new Collection { Id = "c1aaaaaaaa", Name = "C", Items = [new CollectionItem { Id = "i1aaaaaaaa", Name = "R", Request = new ApiRequest { Name = "R", Url = "u" } }] }],
        Environments = [new ApiEnvironment { Id = "e1aaaaaaaa", Name = "E" }],
        ActiveEnvironmentId = activeId,
        Globals = [new("g", "1")],
        History = [new HistoryEntry(new ApiRequest { Url = "h" }, 200, 1, DateTimeOffset.UnixEpoch, "c1aaaaaaaa")],
    };

    private WorkspaceStore Loaded()
    {
        var store = new WorkspaceStore(_dir);
        store.Load();
        return store;
    }

    [Fact]
    public void Fresh_start_is_empty()
    {
        var result = new WorkspaceStore(_dir).Load();
        Assert.Empty(result.Workspace.Collections);
        Assert.Empty(result.Issues);
    }

    [Fact]
    public void Round_trips_and_keeps_personal_data_out_of_the_shared_folder()
    {
        var store = Loaded();
        store.Save(Sample());

        var loaded = new WorkspaceStore(_dir).Load().Workspace;
        Assert.Equal("C", loaded.Collections[0].Name);
        Assert.Equal("e1aaaaaaaa", loaded.ActiveEnvironmentId);
        Assert.Equal("c1aaaaaaaa", loaded.History[0].CollectionId);

        Assert.True(File.Exists(Path.Combine(_dir, "history.json")));
        Assert.True(File.Exists(Path.Combine(_dir, "state.json")));
        var shared = Directory.EnumerateFiles(store.FolderPath, "*", SearchOption.AllDirectories).Select(Path.GetFileName).ToList();
        Assert.DoesNotContain("history.json", shared);
        Assert.DoesNotContain("state.json", shared);
        Assert.DoesNotContain("secrets.dat", shared);
    }

    [Fact]
    public void A_renamed_folder_saves_under_its_new_name_and_keeps_its_requests()
    {
        var request = new CollectionItem { Name = "Get", Request = new ApiRequest { Name = "Get", Url = "u" } };
        var folder = new CollectionItem { Name = "Old name", Children = [request] };
        var collection = new Collection { Name = "C", Items = [folder] };
        var store = Loaded();
        store.Save(new Workspace { Collections = [collection] });

        var renamed = collection with { Items = [folder with { Name = "New name" }] };
        store.Save(new Workspace { Collections = [renamed] });

        var reloaded = new WorkspaceStore(_dir).Load().Workspace.Collections.Single();
        var item = Assert.Single(reloaded.Items);
        Assert.Equal("New name", item.Name);
        Assert.Equal("Get", Assert.Single(item.Children).Name);
    }

    [Fact]
    public void A_new_workspace_gets_a_gitignore_that_excludes_personal_files()
    {
        var store = Loaded();
        store.Save(Sample());

        var text = File.ReadAllText(Path.Combine(store.FolderPath, WorkspaceFolder.GitIgnoreFileName));
        Assert.Contains("state.json", text);
        Assert.Contains("secrets.dat", text);
        Assert.Contains("*.tmp", text);
    }

    [Fact]
    public void An_existing_gitignore_is_never_overwritten()
    {
        var store = Loaded();
        var path = Path.Combine(store.FolderPath, WorkspaceFolder.GitIgnoreFileName);
        File.WriteAllText(path, "my-own-rule\n");

        store.Save(Sample());

        Assert.Equal("my-own-rule\n", File.ReadAllText(path));
    }

    [Fact]
    public void Edits_to_the_gitignore_survive_later_saves()
    {
        var store = Loaded();
        store.Save(Sample());
        var path = Path.Combine(store.FolderPath, WorkspaceFolder.GitIgnoreFileName);
        File.AppendAllText(path, "scratch/\n");

        store.Save(Sample());

        Assert.EndsWith("scratch/\n", File.ReadAllText(path));
    }

    [Fact]
    public void A_gitignore_the_user_deleted_is_not_recreated()
    {
        var store = Loaded();
        store.Save(Sample());
        var path = Path.Combine(store.FolderPath, WorkspaceFolder.GitIgnoreFileName);
        File.Delete(path);

        store.Save(Sample());

        Assert.False(File.Exists(path));
    }

    [Fact]
    public void Theme_choice_is_kept_across_restarts_and_saves()
    {
        var store = Loaded();
        Assert.Null(store.Theme);

        store.SetTheme("light");
        store.Save(Sample());

        var reopened = new WorkspaceStore(_dir);
        reopened.Load();
        Assert.Equal("light", reopened.Theme);
        Assert.Equal("e1aaaaaaaa", new WorkspaceStore(_dir).Load().Workspace.ActiveEnvironmentId);
    }

    [Fact]
    public void Original_single_file_is_migrated_then_kept_as_a_backup()
    {
        var legacy = Path.Combine(_dir, "workspace.json");
        File.WriteAllText(legacy, """
            { "collections": [ { "id": "c1", "name": "Old", "items": [ { "id": "i1", "name": "R", "request": { "id": "r1", "name": "R", "url": "u" } } ] } ],
              "environments": [ { "id": "e1", "name": "E" } ], "activeEnvironmentId": "e1",
              "globals": [ { "key": "a", "value": "1", "enabled": true } ],
              "history": [ { "request": { "id": "h", "url": "u" }, "statusCode": 200, "elapsedMs": 1, "at": "2026-11-04T00:00:00+00:00" } ] }
            """);

        var store = new WorkspaceStore(_dir);
        var first = store.Load();
        Assert.True(first.NeedsSave);
        Assert.Equal("Old", first.Workspace.Collections[0].Name);
        Assert.True(File.Exists(legacy)); // Nothing moves until the new files are safely written.

        store.Save(first.Workspace);
        Assert.False(File.Exists(legacy));
        Assert.True(File.Exists(legacy + ".bak"));
        Assert.Contains(store.FindPlainTextCopies(), p => p == legacy + ".bak");

        var second = new WorkspaceStore(_dir).Load();
        Assert.False(second.NeedsSave);
        Assert.Equal("Old", second.Workspace.Collections[0].Name);
        Assert.Equal("e1", second.Workspace.ActiveEnvironmentId);
        Assert.Single(second.Workspace.History);
    }

    [Fact]
    public void Deleting_copies_removes_only_those_copies()
    {
        var oldAppFolder = Path.Combine(_dir, "PostmanLight");
        Directory.CreateDirectory(oldAppFolder);
        var oldCopy = Path.Combine(oldAppFolder, "workspace.json");
        File.WriteAllText(oldCopy, "{}");
        var backup = Path.Combine(_dir, "workspace.json.bak");
        File.WriteAllText(backup, "{}");
        var unrelated = Path.Combine(_dir, "workspace.json");
        File.WriteAllText(unrelated, "{}");
        File.WriteAllText(Path.Combine(_dir, "state.json"), "{}"); // Already migrated, so workspace.json is left alone.
        var store = new WorkspaceStore(_dir, oldCopy);
        store.Load();
        store.Save(Sample());
        Assert.Equal([oldCopy, backup], store.FindPlainTextCopies());

        Assert.Empty(store.DeletePlainTextCopies());

        Assert.Empty(store.FindPlainTextCopies());
        Assert.False(File.Exists(oldCopy));
        Assert.False(Directory.Exists(oldAppFolder));
        Assert.False(File.Exists(backup));
        Assert.True(File.Exists(unrelated));
        Assert.Equal("C", new WorkspaceStore(_dir).Load().Workspace.Collections[0].Name);
    }

    [Fact]
    public void Old_app_folder_with_other_files_is_kept()
    {
        var oldAppFolder = Path.Combine(_dir, "PostmanLight");
        Directory.CreateDirectory(oldAppFolder);
        var oldCopy = Path.Combine(oldAppFolder, "workspace.json");
        File.WriteAllText(oldCopy, "{}");
        var other = Path.Combine(oldAppFolder, "notes.txt");
        File.WriteAllText(other, "keep");
        var store = Loaded();
        store = new WorkspaceStore(_dir, oldCopy);

        store.DeletePlainTextCopies();

        Assert.False(File.Exists(oldCopy));
        Assert.Equal("keep", File.ReadAllText(other));
    }

    [Fact]
    public void A_store_without_an_old_app_path_never_reports_or_deletes_outside_its_folder()
    {
        var store = new WorkspaceStore(_dir);

        Assert.Empty(store.FindPlainTextCopies());
        Assert.Empty(store.DeletePlainTextCopies());
    }

    [Fact]
    public void A_copy_in_use_is_reported_and_kept()
    {
        var backup = Path.Combine(_dir, "workspace.json.bak");
        File.WriteAllText(backup, "{}");
        var store = new WorkspaceStore(_dir);

        using (new FileStream(backup, FileMode.Open, FileAccess.Read, FileShare.None))
        {
            var problems = store.DeletePlainTextCopies();
            if (OperatingSystem.IsWindows())
                Assert.Contains(backup, Assert.Single(problems));
        }
    }

    [Fact]
    public void Migration_handles_the_very_first_file_format()
    {
        File.WriteAllText(Path.Combine(_dir, "workspace.json"), """
            { "saved": [ { "id": "r1", "name": "Ping", "url": "http://x.test" } ], "variables": [ { "key": "a", "value": "1" } ] }
            """);

        var store = new WorkspaceStore(_dir);
        var loaded = store.Load().Workspace;
        store.Save(loaded);

        var again = new WorkspaceStore(_dir).Load().Workspace;
        Assert.Equal(Workspace.DefaultCollectionName, again.Collections[0].Name);
        Assert.Equal("r1", again.Collections[0].Items[0].Request!.Id);
        Assert.Equal("a", again.Globals[0].Key);
    }

    [Fact]
    public void Corrupt_personal_files_are_set_aside_and_reported()
    {
        File.WriteAllText(Path.Combine(_dir, "history.json"), "{broken");

        var result = new WorkspaceStore(_dir).Load();

        Assert.Contains(result.Issues, i => i.Path.Contains(".corrupt-"));
        var backup = Assert.Single(Directory.GetFiles(_dir, "*.corrupt-*"));
        Assert.Equal("{broken", File.ReadAllText(backup));
    }

    [Fact]
    public void Opening_an_empty_folder_copies_the_workspace_and_leaves_the_original()
    {
        var store = Loaded();
        store.Save(Sample());
        var original = store.FolderPath;
        var target = Path.Combine(_dir, "repo");
        Directory.CreateDirectory(target);
        File.WriteAllText(Path.Combine(target, "README.md"), "existing");

        var (result, copied) = store.OpenFolder(target, Sample());

        Assert.True(copied);
        Assert.Equal("C", result.Workspace.Collections[0].Name);
        Assert.Equal(target, store.FolderPath);
        Assert.True(Directory.EnumerateFiles(original, "*.json", SearchOption.AllDirectories).Any());
        Assert.Equal("existing", File.ReadAllText(Path.Combine(target, "README.md")));

        var reopened = new WorkspaceStore(_dir);
        reopened.Load();
        Assert.Equal(target, reopened.FolderPath);
    }

    [Fact]
    public void Opening_an_existing_workspace_loads_it_and_does_not_overwrite_it()
    {
        var other = Path.Combine(_dir, "other");
        var seed = new WorkspaceFolder(other);
        seed.Load();
        seed.Save(new Workspace { Collections = [new Collection { Id = "c9aaaaaaaa", Name = "Theirs" }] });

        var store = Loaded();
        store.Save(Sample());

        var (result, copied) = store.OpenFolder(other, Sample());

        Assert.False(copied);
        Assert.Equal("Theirs", Assert.Single(result.Workspace.Collections).Name);
        Assert.Equal(seed.Id, store.WorkspaceId);
        Assert.Single(result.Workspace.History); // History is personal, so it stays.
    }

    [Fact]
    public void Each_copy_of_a_workspace_gets_its_own_id()
    {
        var store = Loaded();
        store.Save(Sample());
        var firstId = store.WorkspaceId;
        var target = Path.Combine(_dir, "repo");

        store.OpenFolder(target, Sample());

        Assert.NotEqual(firstId, store.WorkspaceId);
    }

    [Fact]
    public void Missing_workspace_folder_is_reported_and_not_recreated()
    {
        var gone = Path.Combine(_dir, "unplugged");
        File.WriteAllText(Path.Combine(_dir, "state.json"), $$"""{ "workspaceFolder": {{System.Text.Json.JsonSerializer.Serialize(gone)}} }""");

        var store = new WorkspaceStore(_dir);
        var result = store.Load();

        Assert.Contains(result.Issues, i => i.Path == gone);
        Assert.False(Directory.Exists(gone));
        Assert.NotEqual(gone, store.FolderPath);
    }
}

public class SecretsPerWorkspaceTests
{
    [Fact]
    public void Each_workspace_keeps_its_own_secrets()
    {
        var vault = WorkspaceSecrets.WithWorkspace(new Dictionary<string, string>(), "one", new Dictionary<string, string> { ["k"] = "1" });
        vault = WorkspaceSecrets.WithWorkspace(vault, "two", new Dictionary<string, string> { ["k"] = "2" });

        Assert.Equal("1", WorkspaceSecrets.ForWorkspace(vault, "one")["k"]);
        Assert.Equal("2", WorkspaceSecrets.ForWorkspace(vault, "two")["k"]);
    }

    [Fact]
    public void Removing_a_secret_only_affects_its_own_workspace()
    {
        var vault = WorkspaceSecrets.WithWorkspace(new Dictionary<string, string>(), "one", new Dictionary<string, string> { ["k"] = "1" });
        vault = WorkspaceSecrets.WithWorkspace(vault, "two", new Dictionary<string, string> { ["k"] = "2" });

        vault = WorkspaceSecrets.WithWorkspace(vault, "two", new Dictionary<string, string>());

        Assert.Empty(WorkspaceSecrets.ForWorkspace(vault, "two"));
        Assert.Equal("1", WorkspaceSecrets.ForWorkspace(vault, "one")["k"]);
    }

    [Fact]
    public void Entries_from_before_workspaces_had_ids_are_adopted_on_the_next_save()
    {
        var old = new Dictionary<string, string> { ["variable/globals/a"] = "1" };

        var secrets = WorkspaceSecrets.ForWorkspace(old, "one");
        Assert.Equal("1", secrets["variable/globals/a"]);

        var saved = WorkspaceSecrets.WithWorkspace(old, "one", secrets);
        Assert.Equal(["workspace/one/variable/globals/a"], saved.Keys);
    }
}
