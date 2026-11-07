using System.Text.RegularExpressions;

namespace Ricochet.Core;

/// <summary>
/// Turns simple Postman test checks into Ricochet assertions on import. See docs/adr/0037-translate-simple-postman-test-checks-on-import.md.
///
/// A script is translated only when every line of it is a recognised check. Otherwise nothing from it is translated,
/// so a partly understood test never looks fully converted, and the importer keeps the script as it always has.
///
/// Recognised checks, each on its own line, or inside <c>pm.test("name", function () { ... });</c> or an arrow function:
/// <code>
/// pm.response.to.have.status(200)                          status equals 200
/// pm.expect(pm.response.code).to.eql(200)                  status equals 200 (equal, eq and be.equal also work)
/// pm.expect(pm.response.code === 200).to.be.true           status equals 200
/// pm.expect(pm.response.responseTime).to.be.below(300)     time is less than 300 ms (lessThan also works)
/// pm.response.to.have.header("X-Id")                       header exists
/// pm.response.to.have.header("X-Id", "abc")                header equals abc
/// pm.expect(pm.response.json().items[0].id).to.eql("x")    JSON value at items[0].id equals "x"
/// const data = pm.response.json();                         then pm.expect(data.items[0].id).to.eql(1)
/// </code>
/// JSON values are compared as text in Ricochet, so the string "1" also matches the number 1, where Postman's eql does not.
/// </summary>
public static partial class PostmanTestTranslator
{
    private const int MaxShownLength = 100;

    [GeneratedRegex(@"^pm\.test\(\s*(?:""[^""\\]*""|'[^'\\]*')\s*,\s*(?:function\s*\(\s*\)|\(\s*\)\s*=>)\s*\{$")]
    private static partial Regex TestStart();

    [GeneratedRegex(@"^\}\)$")]
    private static partial Regex TestEnd();

    [GeneratedRegex(@"^(?:const|let|var)\s+([A-Za-z_]\w*)\s*=\s*pm\.response\.json\(\)$")]
    private static partial Regex JsonVariable();

    [GeneratedRegex(@"^pm\.response\.to\.have\.status\((\d{3})\)$")]
    private static partial Regex HasStatus();

    [GeneratedRegex(@"^pm\.expect\(pm\.response\.code\)\.to\.(?:be\.)?(?:eql|equal|eq)\((\d{3})\)$")]
    private static partial Regex CodeEquals();

    [GeneratedRegex(@"^pm\.expect\(pm\.response\.code === (\d{3})\)\.to\.be\.true$")]
    private static partial Regex CodeIsTrue();

    [GeneratedRegex(@"^pm\.expect\(pm\.response\.responseTime\)\.to\.(?:be\.)?(?:below|lessThan)\((\d+)\)$")]
    private static partial Regex TimeBelow();

    [GeneratedRegex(@"^pm\.response\.to\.have\.header\(((?:""[^""\\]*""|'[^'\\]*'))(?:,\s*((?:""[^""\\]*""|'[^'\\]*')))?\)$")]
    private static partial Regex HasHeader();

    [GeneratedRegex(@"^pm\.expect\((?:pm\.response\.json\(\)|([A-Za-z_]\w*))\.([A-Za-z_]\w*(?:\.[A-Za-z_]\w*|\[\d+\])*)\)\.to\.(?:be\.)?(?:eql|equal|eq)\((""[^""\\]*""|'[^'\\]*'|-?\d+(?:\.\d+)?)\)$")]
    private static partial Regex JsonEquals();

    /// <summary>
    /// Returns true only when every line is a recognised check. Otherwise <paramref name="notTranslated"/> holds the first line
    /// that was not, and <paramref name="assertions"/> is empty.
    /// </summary>
    public static bool TryTranslate(string script, out List<RequestAssertion> assertions, out string? notTranslated)
    {
        assertions = [];
        notTranslated = null;
        string? jsonVariable = null;
        var inTest = false;

        foreach (var raw in script.Split('\n'))
        {
            var line = raw.Trim();
            if (line.Length == 0 || line.StartsWith("//", StringComparison.Ordinal))
                continue;

            var statement = line.EndsWith(';') ? line[..^1].TrimEnd() : line;
            if (!inTest && TestStart().IsMatch(statement))
            {
                inTest = true;
            }
            else if (inTest && TestEnd().IsMatch(statement))
            {
                inTest = false;
            }
            else if (JsonVariable().Match(statement) is { Success: true } declared)
            {
                jsonVariable = declared.Groups[1].Value;
            }
            else if (Check(statement, jsonVariable) is { } assertion)
            {
                assertions.Add(assertion);
            }
            else
            {
                return Fail(out assertions, out notTranslated, Shorten(line));
            }
        }

        if (inTest)
            return Fail(out assertions, out notTranslated, "pm.test without its closing });");
        if (assertions.Count == 0)
            return Fail(out assertions, out notTranslated, "no checks in it");

        return true;
    }

    private static bool Fail(out List<RequestAssertion> assertions, out string? notTranslated, string reason)
    {
        assertions = [];
        notTranslated = reason;
        return false;
    }

    private static RequestAssertion? Check(string statement, string? jsonVariable)
    {
        if (HasStatus().Match(statement) is { Success: true } status)
            return Status(status.Groups[1].Value);

        if (CodeEquals().Match(statement) is { Success: true } code)
            return Status(code.Groups[1].Value);

        if (CodeIsTrue().Match(statement) is { Success: true } codeTrue)
            return Status(codeTrue.Groups[1].Value);

        if (TimeBelow().Match(statement) is { Success: true } time)
            return new RequestAssertion { Source = AssertionSources.Time, Op = AssertionOps.LessThan, Expected = time.Groups[1].Value };

        if (HasHeader().Match(statement) is { Success: true } header)
        {
            var name = Unquote(header.Groups[1].Value);
            return header.Groups[2].Success
                ? new RequestAssertion { Source = AssertionSources.Header, Path = name, Op = AssertionOps.Equal, Expected = Unquote(header.Groups[2].Value) }
                : new RequestAssertion { Source = AssertionSources.Header, Path = name, Op = AssertionOps.Exists };
        }

        if (JsonEquals().Match(statement) is { Success: true } json)
        {
            // A variable other than the one declared from pm.response.json() is not something we can follow.
            if (json.Groups[1].Success && json.Groups[1].Value != jsonVariable)
                return null;

            return new RequestAssertion
            {
                Source = AssertionSources.Json,
                Path = json.Groups[2].Value,
                Op = AssertionOps.Equal,
                Expected = Unquote(json.Groups[3].Value),
            };
        }

        return null;
    }

    private static RequestAssertion Status(string code) =>
        new() { Source = AssertionSources.Status, Op = AssertionOps.Equal, Expected = code };

    private static string Unquote(string text) =>
        text.Length >= 2 && (text[0] is '"' or '\'') ? text[1..^1] : text;

    private static string Shorten(string text) =>
        text.Length <= MaxShownLength ? text : text[..MaxShownLength] + "…";
}
