using System.Text;
using System.Text.RegularExpressions;

namespace Ricochet.Core;

/// <summary>The Variables read from a .env file, and the lines that could not be used.</summary>
public sealed record DotEnvFile(IReadOnlyList<KeyValue> Variables, IReadOnlyList<string> Problems);

/// <summary>
/// Reads the optional .env file in a Workspace folder (ADR 0021). Its values are personal, so they are always Secrets:
/// they are masked in output, and they never reach a Workspace file, because nothing here returns them as part of one.
/// </summary>
public static partial class DotEnv
{
    public const string FileName = ".env";

    [GeneratedRegex(@"^[A-Za-z0-9_.-]+$")]
    private static partial Regex ValidName();

    /// <summary>Reads the file in the folder. A missing file means no values and no problems.</summary>
    public static DotEnvFile Read(string folderRoot)
    {
        var path = Path.Combine(folderRoot, FileName);
        if (!File.Exists(path))
            return new DotEnvFile([], []);

        try
        {
            return Parse(File.ReadAllText(path));
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            return new DotEnvFile([], [$"{FileName} could not be read: {ex.Message}"]);
        }
    }

    /// <summary>
    /// Parses KEY=VALUE lines. Blank lines and lines starting with # are skipped, and a leading "export " is allowed.
    /// Quoted values may span lines; double quotes understand \n, \r, \t, \" and \\; single quotes are literal.
    /// An unquoted value ends at a " #" comment. A repeated name takes its last value. Nothing is expanded.
    /// </summary>
    public static DotEnvFile Parse(string text)
    {
        var variables = new List<KeyValue>();
        var positions = new Dictionary<string, int>(StringComparer.Ordinal);
        var problems = new List<string>();

        var source = text.TrimStart('﻿');
        var pos = 0;
        var line = 1;

        while (pos < source.Length)
        {
            var lineEnd = IndexOfNewline(source, pos);
            var rawLine = source[pos..lineEnd];
            var startLine = line;
            var content = rawLine.Trim();

            // Move past this line's newline. Quoted values below may move further.
            var next = Math.Min(lineEnd + 1, source.Length);
            var nextLine = line + 1;

            if (content.Length == 0 || content.StartsWith('#'))
            {
                pos = next;
                line = nextLine;
                continue;
            }

            if (content.StartsWith("export ", StringComparison.Ordinal) || content.StartsWith("export\t", StringComparison.Ordinal))
                content = content[7..].TrimStart();

            var equals = content.IndexOf('=');
            var key = equals < 0 ? "" : content[..equals].Trim();
            if (equals < 0 || key.Length == 0)
            {
                problems.Add($"Line {startLine} is not KEY=VALUE, so it was skipped.");
                pos = next;
                line = nextLine;
                continue;
            }

            if (!ValidName().IsMatch(key))
            {
                problems.Add($"Line {startLine}: \"{key}\" is not a usable name, so it was skipped.");
                pos = next;
                line = nextLine;
                continue;
            }

            // The value starts after the '=' in the source, so a quoted value can read the lines that follow.
            // The first '=' in the line is the separator, because a name can't contain one and "export" doesn't either.
            var valueStart = pos + rawLine.IndexOf('=') + 1;
            var afterEquals = SkipBlanks(source, valueStart);
            string value;

            if (afterEquals < source.Length && source[afterEquals] is '"' or '\'')
            {
                var (quoted, closeIndex, closed) = ReadQuoted(source, afterEquals);
                if (!closed)
                {
                    problems.Add($"Line {startLine}: the quote around \"{key}\" is never closed, so it was skipped.");
                    pos = next;
                    line = nextLine;
                    continue;
                }

                var trailing = source[(closeIndex + 1)..IndexOfNewline(source, closeIndex + 1)].Trim();
                if (trailing.Length > 0 && !trailing.StartsWith('#'))
                    problems.Add($"Line {startLine}: text after the closing quote of \"{key}\" was ignored.");

                value = quoted;
                lineEnd = IndexOfNewline(source, closeIndex);
                next = Math.Min(lineEnd + 1, source.Length);
                nextLine = line + CountNewlines(source, pos, next);
            }
            else
            {
                var unquoted = source[afterEquals..lineEnd];
                value = StripComment(unquoted).TrimEnd();
            }

            var entry = new KeyValue(key, value, IsSecret: true);
            if (positions.TryGetValue(key, out var index))
                variables[index] = entry;
            else
            {
                positions[key] = variables.Count;
                variables.Add(entry);
            }

            pos = next;
            line = nextLine;
        }

        return new DotEnvFile(variables, problems);
    }

    private static int IndexOfNewline(string text, int from)
    {
        var index = text.IndexOf('\n', from);
        return index < 0 ? text.Length : index;
    }

    private static int SkipBlanks(string text, int index)
    {
        while (index < text.Length && text[index] is ' ' or '\t')
            index++;
        return index;
    }

    private static int CountNewlines(string text, int from, int to)
    {
        var count = 0;
        for (var i = from; i < to && i < text.Length; i++)
        {
            if (text[i] == '\n')
                count++;
        }

        return count;
    }

    /// <summary>Reads a quoted value that starts at <paramref name="open"/>. Returns the value and the index of the closing quote.</summary>
    private static (string Value, int CloseIndex, bool Closed) ReadQuoted(string text, int open)
    {
        var quote = text[open];
        var value = new StringBuilder();
        var i = open + 1;

        while (i < text.Length)
        {
            var c = text[i];
            if (c == quote)
                return (value.ToString(), i, true);

            if (quote == '"' && c == '\\' && i + 1 < text.Length)
            {
                var escaped = text[i + 1];
                switch (escaped)
                {
                    case 'n': value.Append('\n'); break;
                    case 'r': value.Append('\r'); break;
                    case 't': value.Append('\t'); break;
                    case '"': value.Append('"'); break;
                    case '\\': value.Append('\\'); break;
                    default: value.Append(c).Append(escaped); break;
                }

                i += 2;
                continue;
            }

            value.Append(c);
            i++;
        }

        return (value.ToString(), text.Length, false);
    }

    /// <summary>Removes a comment that follows whitespace, such as "value # note". A # inside a value is kept.</summary>
    private static string StripComment(string value)
    {
        for (var i = 0; i < value.Length; i++)
        {
            if (value[i] == '#' && (i == 0 || value[i - 1] is ' ' or '\t'))
                return value[..i];
        }

        return value;
    }
}
