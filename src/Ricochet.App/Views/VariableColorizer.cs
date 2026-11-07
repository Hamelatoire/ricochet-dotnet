using Avalonia.Media;
using AvaloniaEdit.Document;
using AvaloniaEdit.Rendering;
using Ricochet.Core;

namespace Ricochet.App.Views;

/// <summary>
/// Draws each placeholder as a chip (ADR 0009): a tinted background, in a warning colour when its name has no value to send.
/// The text is unchanged, so the chip is only how it looks on screen.
/// </summary>
public sealed class VariableColorizer(Func<string, IReadOnlyList<Chip>> chipsIn) : DocumentColorizingTransformer
{
    private static readonly IBrush ResolvedText = new SolidColorBrush(Color.Parse("#FF7A50"));
    private static readonly IBrush ResolvedChip = new SolidColorBrush(Color.Parse("#26FF7A50"));
    private static readonly IBrush MissingText = new SolidColorBrush(Color.Parse("#E5484D"));
    private static readonly IBrush MissingChip = new SolidColorBrush(Color.Parse("#26E5484D"));

    protected override void ColorizeLine(DocumentLine line)
    {
        var text = CurrentContext.Document.GetText(line);
        foreach (var chip in chipsIn(text))
        {
            var start = line.Offset + chip.Start;
            var textBrush = chip.Resolves ? ResolvedText : MissingText;
            var chipBrush = chip.Resolves ? ResolvedChip : MissingChip;
            ChangeLinePart(start, start + chip.Length, element =>
            {
                element.TextRunProperties.SetForegroundBrush(textBrush);
                element.TextRunProperties.SetBackgroundBrush(chipBrush);
            });
        }
    }
}
