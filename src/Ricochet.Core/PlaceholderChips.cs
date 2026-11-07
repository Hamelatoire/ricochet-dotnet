using System.Text.RegularExpressions;

namespace Ricochet.Core;

/// <summary>A {{...}} placeholder in a field's text, which the app draws as a chip (ADR 0009).</summary>
/// <param name="Start">Offset of the opening braces in the field's text.</param>
/// <param name="Length">Length of the whole placeholder, braces included.</param>
/// <param name="Name">The name between the braces, trimmed.</param>
/// <param name="IsFunction">True for a built-in function (ADR 0008), false for a Variable.</param>
/// <param name="Resolves">True when the name has a value to send: a Variable in scope, or a function that runs.</param>
public sealed record Chip(int Start, int Length, string Name, bool IsFunction, bool Resolves)
{
    public int End => Start + Length;
}

/// <summary>A name offered after typing "{{" (ADR 0009).</summary>
public sealed record ChipSuggestion(string Name, bool IsFunction)
{
    /// <summary>The word the suggestion list shows beside the name.</summary>
    public string Kind => IsFunction ? "function" : "Variable";
}

/// <summary>
/// Finds the placeholders the editor shows as chips, what each would resolve to, and what to suggest. The text itself is
/// never changed here: a chip exists only on screen, so the file and the sent Request keep the plain {{name}} text.
/// Resolving follows <see cref="VariableResolver.Resolve"/>: a Variable wins over a function of the same name.
/// </summary>
public static partial class PlaceholderChips
{
    [GeneratedRegex(@"\{\{([^{}\r\n]+)\}\}")]
    private static partial Regex Pattern();

    /// <summary>The chips in a field's text, in order. <paramref name="variables"/> is every scope layered lowest first.</summary>
    public static IReadOnlyList<Chip> Find(string text, IEnumerable<KeyValue> variables)
    {
        var names = ResolvableNames(variables);
        var chips = new List<Chip>();
        foreach (Match match in Pattern().Matches(text))
        {
            var name = match.Groups[1].Value.Trim();
            var isVariable = names.Contains(name);
            var isFunction = !isVariable && IsFunction(name);
            var resolves = isVariable || (isFunction && BuiltInFunctions.TryEvaluate(name) is not null);
            chips.Add(new Chip(match.Index, match.Length, name, isFunction, resolves));
        }

        return chips;
    }

    /// <summary>
    /// What to offer after "{{" for the text typed so far: Variables in scope first, then the built-in functions.
    /// Disabled Variables, and Secrets with no value, are left out because they would not resolve.
    /// </summary>
    public static IReadOnlyList<ChipSuggestion> Suggest(IEnumerable<KeyValue> variables, string typed)
    {
        var prefix = typed.Trim();
        var suggestions = new List<ChipSuggestion>();
        var seen = new HashSet<string>(StringComparer.Ordinal);
        foreach (var variable in variables.Where(IsResolvable))
        {
            var name = variable.Key.Trim();
            if (name.StartsWith(prefix, StringComparison.OrdinalIgnoreCase) && seen.Add(name))
                suggestions.Add(new ChipSuggestion(name, false));
        }

        suggestions.AddRange(BuiltInFunctions.Examples
            .Where(example => example.StartsWith(prefix, StringComparison.OrdinalIgnoreCase))
            .Select(example => new ChipSuggestion(example, true)));
        return suggestions;
    }

    /// <summary>
    /// The text shown when the pointer rests on a chip: the value, or only that a Secret is hidden. A Secret's value is
    /// never returned (ADR 0009).
    /// </summary>
    public static string Tip(Chip chip, IEnumerable<KeyValue> variables)
    {
        var braces = "{{" + chip.Name + "}}";
        if (chip.IsFunction)
            return chip.Resolves ? $"{braces}: built-in function, a new value on each send" : $"{braces}: unknown function";

        // The same Variable the send uses: the last resolvable entry with this name.
        var match = variables.Where(IsResolvable).LastOrDefault(v => v.Key.Trim() == chip.Name);
        if (match is null)
            return $"{braces}: no value";
        if (match.IsSecret)
            return $"{braces}: Secret, hidden";
        return $"{braces} = {match.Value}";
    }

    /// <summary>The chip the offset falls strictly inside, if any.</summary>
    public static Chip? Covering(IReadOnlyList<Chip> chips, int offset) =>
        chips.FirstOrDefault(chip => chip.Start < offset && offset < chip.End);

    /// <summary>The chip that ends exactly at the offset, if any.</summary>
    public static Chip? EndingAt(IReadOnlyList<Chip> chips, int offset) =>
        chips.FirstOrDefault(chip => chip.End == offset);

    /// <summary>The chip that starts exactly at the offset, if any.</summary>
    public static Chip? StartingAt(IReadOnlyList<Chip> chips, int offset) =>
        chips.FirstOrDefault(chip => chip.Start == offset);

    private static bool IsFunction(string name) => name.Contains('(') || BuiltInFunctions.IsPostmanAlias(name);

    // The same rule VariableResolver.Resolve applies: enabled, and a Secret with no value counts as unset.
    private static bool IsResolvable(KeyValue variable) =>
        variable.Enabled && variable.Key.Length > 0 && !(variable.IsSecret && variable.Value.Length == 0);

    private static HashSet<string> ResolvableNames(IEnumerable<KeyValue> variables) =>
        variables.Where(IsResolvable).Select(variable => variable.Key.Trim()).ToHashSet(StringComparer.Ordinal);
}
