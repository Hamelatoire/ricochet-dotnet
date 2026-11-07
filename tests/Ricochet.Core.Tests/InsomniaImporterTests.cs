using Ricochet.Core;

namespace Ricochet.Core.Tests;

public class InsomniaImporterTests
{
    // A made-up v4 export: one workspace with a base environment, a sub-environment, a folder and loose requests.
    private const string Export = """
        {
          "_type": "export",
          "__export_format": 4,
          "__export_source": "insomnia.desktop.app:v0.0.0",
          "resources": [
            { "_id": "wrk_1", "_type": "workspace", "name": "Shop", "description": "", "metaSortKey": 1 },
            { "_id": "env_base", "_type": "environment", "parentId": "wrk_1", "name": "Base Environment",
              "data": { "base_url": "https://api.example.test", "page_size": 20 }, "metaSortKey": 2 },
            { "_id": "env_dev", "_type": "environment", "parentId": "env_base", "name": "Dev",
              "data": { "base_url": "https://dev.example.test" }, "metaSortKey": 3 },
            { "_id": "fld_orders", "_type": "request_group", "parentId": "wrk_1", "name": "Orders", "metaSortKey": 5,
              "headers": [ { "name": "X-Client", "value": "test-client" } ],
              "authentication": { "type": "bearer", "token": "{{ _.api_token }}" } },
            { "_id": "req_list", "_type": "request", "parentId": "fld_orders", "name": "List orders", "metaSortKey": 6,
              "method": "GET", "url": "{{ _.base_url }}/orders", "headers": [], "parameters": [
                { "name": "page", "value": "1" },
                { "name": "debug", "value": "1", "disabled": true } ],
              "authentication": { "type": "inherit" } },
            { "_id": "req_create", "_type": "request", "parentId": "fld_orders", "name": "Create order", "metaSortKey": 7,
              "method": "POST", "url": "{{ _.base_url }}/orders", "description": "Makes an order",
              "headers": [ { "name": "X-Client", "value": "request-client" }, { "name": "X-Trace", "value": "off", "disabled": true } ],
              "body": { "mimeType": "application/json", "text": "{\"sku\": \"{% uuid 'v4' %}\"}" },
              "authentication": { "type": "none" } },
            { "_id": "req_root", "_type": "request", "parentId": "wrk_1", "name": "Ping", "metaSortKey": 8,
              "method": "get", "url": "{{ _.base_url }}/ping",
              "authentication": { "type": "basic", "username": "tester", "password": "example-password" } },
            { "_id": "grpc_1", "_type": "grpc_request", "parentId": "wrk_1", "name": "Stream", "metaSortKey": 9 }
          ]
        }
        """;

    private static InsomniaImportResult Import(string json = Export) => InsomniaImporter.Import(json, "shop.json");

    private static InsomniaWorkspaceImport OnlyWorkspace(InsomniaImportResult result) => result.Workspaces.Single();

    private static ApiRequest FindRequest(IEnumerable<CollectionItem> items, string name) =>
        items.SelectMany(Flatten).Single(i => i.Name == name).Request!;

    private static IEnumerable<CollectionItem> Flatten(CollectionItem item)
    {
        yield return item;
        foreach (var child in item.Children.SelectMany(Flatten))
            yield return child;
    }

    [Fact]
    public void Imports_workspace_as_collection_with_folders_and_requests_in_sort_order()
    {
        var workspace = OnlyWorkspace(Import());
        var items = workspace.Collection.Items;

        Assert.Equal("Shop", workspace.Collection.Name);
        Assert.Equal(["Orders", "Ping"], items.Select(i => i.Name));
        Assert.Equal(["List orders", "Create order"], items[0].Children.Select(i => i.Name));
        Assert.Equal("POST", FindRequest(items, "Create order").Method);
        Assert.Equal("GET", FindRequest(items, "Ping").Method);
        Assert.Contains("3 requests, 1 folder", Import().Summary);
    }

    [Fact]
    public void Unsupported_resource_types_are_reported_not_imported()
    {
        var result = Import();

        Assert.Equal(["Orders", "Ping"], OnlyWorkspace(result).Collection.Items.Select(i => i.Name));
        Assert.Contains(result.Issues, i => i.Category == ImportCategories.DroppedData && i.Location == "Shop / Stream" && i.Detail == "grpc_request not imported");
    }

    [Fact]
    public void Base_environment_becomes_collection_variables_and_sub_environments_become_environments()
    {
        var workspace = OnlyWorkspace(Import());

        Assert.Equal(["base_url", "page_size"], workspace.Collection.Variables.Select(v => v.Key));
        Assert.Equal("20", workspace.Collection.Variables.Single(v => v.Key == "page_size").Value);

        var environment = Assert.Single(workspace.Environments);
        Assert.Equal("Dev", environment.Name);
        Assert.Equal("https://dev.example.test", environment.Variables.Single().Value);
    }

    [Fact]
    public void Insomnia_placeholders_are_rewritten_to_ours()
    {
        var list = FindRequest(OnlyWorkspace(Import()).Collection.Items, "List orders");

        Assert.Equal("{{base_url}}/orders", list.Url);
        Assert.DoesNotContain("{{ _.", list.Url);
    }

    [Fact]
    public void Folder_headers_apply_unless_the_request_sets_the_same_name()
    {
        var items = OnlyWorkspace(Import()).Collection.Items;

        var list = FindRequest(items, "List orders");
        Assert.Equal([("X-Client", "test-client")], list.Headers.Select(h => (h.Key, h.Value)));

        var create = FindRequest(items, "Create order");
        var clientHeaders = create.Headers.Where(h => h.Enabled && h.Key == "X-Client").ToList();
        Assert.Equal("request-client", Assert.Single(clientHeaders).Value);
        Assert.Contains(create.Headers, h => h.Key == "X-Trace" && !h.Enabled);
    }

    [Fact]
    public void Bearer_is_inherited_from_folder_and_resolved_onto_the_request()
    {
        var list = FindRequest(OnlyWorkspace(Import()).Collection.Items, "List orders");

        Assert.Equal(AuthKinds.Bearer, list.Auth!.Kind);
        Assert.Equal("{{api_token}}", list.Auth.Token);
    }

    [Fact]
    public void Basic_auth_is_imported_and_none_removes_inherited_auth()
    {
        var items = OnlyWorkspace(Import()).Collection.Items;

        var ping = FindRequest(items, "Ping").Auth!;
        Assert.Equal(AuthKinds.Basic, ping.Kind);
        Assert.Equal("tester", ping.Username);
        Assert.Equal("example-password", ping.Password);
        Assert.Null(FindRequest(items, "Create order").Auth);
    }

    [Fact]
    public void Disabled_query_parameters_are_reported()
    {
        var result = Import();

        Assert.Contains(result.Issues, i => i.Category == ImportCategories.DroppedData && i.Location == "Shop / Orders / List orders" && i.Detail == "disabled query parameter \"debug\"");
    }

    [Fact]
    public void Body_is_kept_with_its_content_type()
    {
        var create = FindRequest(OnlyWorkspace(Import()).Collection.Items, "Create order");

        Assert.Equal("{\"sku\": \"{{$guid}}\"}", create.Body);
        Assert.Contains(create.Headers, h => h.Key == "Content-Type" && h.Value == "application/json");
    }

    [Fact]
    public void Form_bodies_are_converted_and_file_fields_are_reported()
    {
        const string json = """
            { "__export_format": 4, "resources": [
              { "_id": "wrk_f", "_type": "workspace", "name": "Forms" },
              { "_id": "req_form", "_type": "request", "parentId": "wrk_f", "name": "Login", "method": "POST", "url": "https://x.example.test/login",
                "body": { "mimeType": "application/x-www-form-urlencoded", "params": [
                  { "name": "user name", "value": "{{ _.user }}" },
                  { "name": "off", "value": "1", "disabled": true } ] } },
              { "_id": "req_upload", "_type": "request", "parentId": "wrk_f", "name": "Upload", "method": "POST", "url": "https://x.example.test/upload",
                "body": { "mimeType": "multipart/form-data", "params": [
                  { "name": "note", "value": "hello" },
                  { "name": "avatar", "type": "file", "fileName": "/home/someone/avatar.png" } ] } },
              { "_id": "req_binary", "_type": "request", "parentId": "wrk_f", "name": "Binary", "method": "PUT", "url": "https://x.example.test/blob",
                "body": { "mimeType": "application/octet-stream", "fileName": "/home/someone/data.bin" } }
            ] }
            """;

        var result = Import(json);
        var items = OnlyWorkspace(result).Collection.Items;

        var login = FindRequest(items, "Login");
        Assert.Equal("user%20name={{user}}", login.Body);
        Assert.Contains(login.Headers, h => h.Key == "Content-Type" && h.Value == "application/x-www-form-urlencoded");

        var upload = FindRequest(items, "Upload");
        Assert.Contains("name=\"note\"", upload.Body);
        Assert.DoesNotContain("avatar", upload.Body);
        Assert.Contains(result.Issues, i => i.Category == ImportCategories.DroppedBody && i.Location == "Forms / Upload" && i.Detail == "file form field \"avatar\"");
        Assert.Contains(result.Issues, i => i.Category == ImportCategories.DroppedBody && i.Location == "Forms / Binary" && i.Detail == "file upload");
        Assert.Equal("", FindRequest(items, "Binary").Body);
    }

    [Fact]
    public void Unknown_placeholders_are_left_as_written_and_listed()
    {
        const string json = """
            { "__export_format": 4, "resources": [
              { "_id": "wrk_d", "_type": "workspace", "name": "Dynamic" },
              { "_id": "req_d", "_type": "request", "parentId": "wrk_d", "name": "Session", "method": "GET",
                "url": "https://x.example.test/{% response 'body', 'req_1', 'b64::$.id::46b', 'never', 60 %}?n={{ uuid() }}" }
            ] }
            """;

        var result = Import(json);
        var request = FindRequest(OnlyWorkspace(result).Collection.Items, "Session");

        Assert.Contains("{% response", request.Url);
        Assert.Contains(result.Issues, i => i.Category == ImportCategories.DynamicVariables && i.Location == "Dynamic / Session");
    }

    [Fact]
    public void Each_workspace_in_an_export_becomes_its_own_collection()
    {
        const string json = """
            { "__export_format": 4, "resources": [
              { "_id": "wrk_a", "_type": "workspace", "name": "Alpha", "metaSortKey": 1 },
              { "_id": "wrk_b", "_type": "workspace", "name": "Beta", "metaSortKey": 2 },
              { "_id": "req_b", "_type": "request", "parentId": "wrk_b", "name": "Only in Beta", "method": "GET", "url": "https://b.example.test" }
            ] }
            """;

        var result = Import(json);

        Assert.Equal(["Alpha", "Beta"], result.Workspaces.Select(w => w.Collection.Name));
        Assert.Empty(result.Workspaces[0].Collection.Items);
        Assert.Equal(["Only in Beta"], result.Workspaces[1].Collection.Items.Select(i => i.Name));
    }

    [Fact]
    public void Descriptions_are_reported_as_not_imported()
    {
        var result = Import();

        Assert.Contains(result.Issues, i => i.Category == ImportCategories.DroppedData && i.Location == "1 description");
    }

    [Fact]
    public void Rejects_a_file_that_is_not_an_insomnia_export()
    {
        Assert.Throws<InsomniaImportException>(() => Import("{ \"info\": {}, \"item\": [] }"));
        Assert.Throws<InsomniaImportException>(() => Import("not json"));
    }

    [Fact]
    public void Rejects_other_export_formats()
    {
        var ex = Assert.Throws<InsomniaImportException>(() => Import("{ \"__export_format\": 3, \"resources\": [] }"));

        Assert.Contains("format 3", ex.Message);
    }

    [Fact]
    public void Rejects_an_export_with_no_workspace()
    {
        Assert.Throws<InsomniaImportException>(() => Import("{ \"__export_format\": 4, \"resources\": [] }"));
    }

    [Fact]
    public void Report_text_groups_issues_by_category()
    {
        var text = Import().ToReportText();

        Assert.StartsWith("Imported workspace \"Shop\"", text);
        Assert.Contains($"{ImportCategories.DroppedData} (", text);
    }
}
