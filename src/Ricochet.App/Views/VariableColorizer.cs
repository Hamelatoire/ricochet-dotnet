using System.Text.RegularExpressions;
using Avalonia.Media;
using AvaloniaEdit.Document;
using AvaloniaEdit.Rendering;

namespace Ricochet.App.Views;

/// <summary>Colors each {{variable}} placeholder, so it stands out from the text around it.</summary>
public sealed class VariableColorizer(IBrush brush) : DocumentColorizingTransformer
{
    private static readonly Regex Placeholder = new(@"\{\{[^{}\r\n]*\}\}", RegexOptions.Compiled);

    protected override void ColorizeLine(DocumentLine line)
    {
        var text = CurrentContext.Document.GetText(line);
        foreach (Match match in Placeholder.Matches(text))
        {
            var start = line.Offset + match.Index;
            ChangeLinePart(start, start + match.Length, element => element.TextRunProperties.SetForegroundBrush(brush));
        }
    }
}
