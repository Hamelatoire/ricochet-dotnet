using System.Globalization;
using System.Text.RegularExpressions;

namespace Ricochet.Core;

/// <summary>
/// Generated values written inside the braces with parentheses, such as {{uuid.v4()}}. The list is fixed, so a shared
/// Collection can't run code, read files or send a Secret anywhere. See docs/adr/0008-built-in-functions-for-generated-values.md.
/// </summary>
public static partial class BuiltInFunctions
{
    /// <summary>Postman's dynamic variable names, which act as these functions so imported requests keep working.</summary>
    private static readonly IReadOnlyDictionary<string, string> PostmanAliases = new Dictionary<string, string>
    {
        ["$guid"] = "uuid.v4()",
        ["$timestamp"] = "timestamp.unix()",
        ["$randomInt"] = "random.int(0, 1000)",
    };

    /// <summary>True for a Postman dynamic variable name that a built-in function replaces, such as <c>$guid</c>.</summary>
    public static bool IsPostmanAlias(string name) => PostmanAliases.ContainsKey(name.Trim());

    [GeneratedRegex(@"^([A-Za-z][A-Za-z0-9]*(?:\.[A-Za-z][A-Za-z0-9]*)+)\((.*)\)$", RegexOptions.Singleline)]
    private static partial Regex Call();

    [GeneratedRegex(@"^-?[0-9]+$")]
    private static partial Regex WholeNumber();

    /// <summary>
    /// Evaluates a placeholder's name, such as <c>uuid.v4()</c>. Returns null when it is not a known function or its
    /// arguments are wrong, so the placeholder stays as written and is reported as missing.
    /// </summary>
    public static string? TryEvaluate(string name)
    {
        var text = name.Trim();
        if (PostmanAliases.TryGetValue(text, out var alias))
            text = alias;

        var match = Call().Match(text);
        if (!match.Success)
            return null;

        var args = ParseArguments(match.Groups[2].Value);
        if (args is null)
            return null;

        var now = DateTimeOffset.UtcNow;
        var invariant = CultureInfo.InvariantCulture;
        try
        {
            return (match.Groups[1].Value, args.Count) switch
            {
                ("uuid.v4", 0) => Guid.NewGuid().ToString(),
                ("timestamp.iso8601", 0) => now.ToString("yyyy-MM-dd'T'HH:mm:ss.fff'Z'", invariant),
                ("timestamp.unix", 0) => now.ToUnixTimeSeconds().ToString(invariant),
                ("timestamp.unixMillis", 0) => now.ToUnixTimeMilliseconds().ToString(invariant),
                ("timestamp.format", 1) when args[0].Length > 0 => now.ToString(args[0], invariant),
                ("random.int", 2) when long.TryParse(args[0], invariant, out var low)
                                       && long.TryParse(args[1], invariant, out var high)
                                       && low <= high && high < long.MaxValue
                    => Random.Shared.NextInt64(low, high + 1).ToString(invariant),
                _ => null,
            };
        }
        catch (FormatException)
        {
            // A format string the framework can't use is reported as missing rather than sent broken.
            return null;
        }
    }

    /// <summary>
    /// Splits the arguments. Text goes in single quotes and numbers are written bare. A variable or another function
    /// can't be an argument, so anything else makes the whole call invalid (null).
    /// </summary>
    private static List<string>? ParseArguments(string text)
    {
        var args = new List<string>();
        if (text.Trim().Length == 0)
            return args;

        var pieces = new List<string>();
        var current = new System.Text.StringBuilder();
        var inQuotes = false;
        foreach (var c in text)
        {
            if (c == '\'')
                inQuotes = !inQuotes;
            if (c == ',' && !inQuotes)
            {
                pieces.Add(current.ToString());
                current.Clear();
            }
            else
            {
                current.Append(c);
            }
        }

        if (inQuotes)
            return null;

        pieces.Add(current.ToString());
        foreach (var piece in pieces)
        {
            var arg = piece.Trim();
            if (arg.Length >= 2 && arg[0] == '\'' && arg[^1] == '\'' && arg[1..^1].IndexOf('\'') < 0)
                args.Add(arg[1..^1]);
            else if (WholeNumber().IsMatch(arg))
                args.Add(arg);
            else
                return null;
        }

        return args;
    }
}
