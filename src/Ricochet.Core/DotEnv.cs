namespace Ricochet.Core;

/// <summary>Reads a .env file in the workspace folder for local runs (ADR 0021). Its values never leave the machine.</summary>
public static class DotEnv
{
    public const string FileName = ".env";

    private static readonly System.Text.RegularExpressions.Regex NameRule = new("^[A-Za-z_][A-Za-z0-9_]*$");

    /// <summary>The variables in the file, and the line numbers of lines that could not be read.</summary>
    public sealed record Result(IReadOnlyList<KeyValue> Variables, IReadOnlyList<int> SkippedLines);

    /// <summary>A missing file is not an error: most workspaces have none.</summary>
    public static Result Read(string workspaceFolder)
    {
        var path = Path.Combine(workspaceFolder, FileName);
        return File.Exists(path) ? Parse(File.ReadAllText(path)) : new Result([], []);
    }

    /// <summary>
    /// Accepts KEY=VALUE lines, with blank lines, # comments, an optional "export " prefix, and values in
    /// matching single or double quotes. Anything else is skipped and reported by line number.
    /// </summary>
    public static Result Parse(string text)
    {
        var variables = new List<KeyValue>();
        var skipped = new List<int>();
        var lines = text.Replace("\r\n", "\n").Split('\n');

        for (var i = 0; i < lines.Length; i++)
        {
            var line = lines[i].Trim();
            if (line.Length == 0 || line.StartsWith('#'))
                continue;

            if (line.StartsWith("export ", StringComparison.Ordinal))
                line = line["export ".Length..].TrimStart();

            var equals = line.IndexOf('=');
            if (equals < 0)
            {
                skipped.Add(i + 1);
                continue;
            }

            var key = line[..equals].Trim();
            if (!NameRule.IsMatch(key))
            {
                skipped.Add(i + 1);
                continue;
            }

            variables.Add(new KeyValue(key, Unquote(line[(equals + 1)..].Trim())));
        }

        return new Result(variables, skipped);
    }

    private static string Unquote(string value)
    {
        if (value.Length >= 2 && (value[0] == '"' || value[0] == '\'') && value[^1] == value[0])
            return value[1..^1];

        return value;
    }
}
