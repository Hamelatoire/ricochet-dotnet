using System.Text;
using System.Text.Json;

namespace Ricochet.Core;

public enum DataFileFormat
{
    Csv,
    Json,
}

/// <summary>
/// Reads a CSV or JSON data file into rows of Variables (ADR 0018). A CSV file has a header row of names;
/// a JSON file is an array of objects. Each returned row is one run's values. Empty rows are skipped.
/// </summary>
public static class DataFileParser
{
    public static DataFileFormat FormatFor(string fileName) =>
        Path.GetExtension(fileName).ToLowerInvariant() switch
        {
            ".csv" => DataFileFormat.Csv,
            ".json" => DataFileFormat.Json,
            _ => throw new RunSetupException($"\"{fileName}\" is not a data file. Use a .csv or .json file."),
        };

    /// <exception cref="RunSetupException">The file is not in the expected shape. The message names the problem.</exception>
    public static IReadOnlyList<IReadOnlyList<KeyValue>> Parse(string text, DataFileFormat format) => format switch
    {
        DataFileFormat.Csv => ParseCsv(text),
        DataFileFormat.Json => ParseJson(text),
        _ => throw new ArgumentOutOfRangeException(nameof(format)),
    };

    private static IReadOnlyList<IReadOnlyList<KeyValue>> ParseCsv(string text)
    {
        var records = ReadCsvRecords(text);
        if (records.Count == 0)
            throw new RunSetupException("The CSV file has no header row.");

        var headers = records[0].Fields.Select(h => h.Trim()).ToList();
        for (var i = 0; i < headers.Count; i++)
        {
            if (headers[i].Length == 0)
                throw new RunSetupException($"Column {i + 1} of the CSV header has no name.");
            if (headers.IndexOf(headers[i]) != i)
                throw new RunSetupException($"The CSV header names \"{headers[i]}\" more than once.");
        }

        var rows = new List<IReadOnlyList<KeyValue>>();
        foreach (var (line, fields) in records.Skip(1))
        {
            if (fields.Count != headers.Count)
                throw new RunSetupException($"Line {line} has {fields.Count} values, but the header has {headers.Count}.");

            rows.Add(headers.Select((name, i) => new KeyValue(name, fields[i])).ToList());
        }

        return rows;
    }

    /// <summary>Reads RFC 4180 records: quoted fields may hold commas, line breaks and doubled quotes. Blank lines are dropped.</summary>
    private static List<(int Line, List<string> Fields)> ReadCsvRecords(string text)
    {
        if (text.StartsWith('﻿'))
            text = text[1..];

        var records = new List<(int, List<string>)>();
        var fields = new List<string>();
        var field = new StringBuilder();
        var inQuotes = false;
        var line = 1;
        var recordLine = 1;
        var quoteLine = 1;

        void EndRecord()
        {
            fields.Add(field.ToString());
            // A row of nothing but blanks is an empty row, and is skipped.
            if (fields.Any(f => f.Trim().Length > 0))
                records.Add((recordLine, fields));
            fields = [];
            field.Clear();
        }

        for (var i = 0; i < text.Length; i++)
        {
            var c = text[i];
            if (inQuotes)
            {
                if (c == '"' && i + 1 < text.Length && text[i + 1] == '"')
                {
                    field.Append('"');
                    i++;
                }
                else if (c == '"')
                {
                    inQuotes = false;
                }
                else
                {
                    if (c == '\n')
                        line++;
                    field.Append(c);
                }
                continue;
            }

            switch (c)
            {
                case '"' when field.Length == 0:
                    inQuotes = true;
                    quoteLine = line;
                    break;
                case ',':
                    fields.Add(field.ToString());
                    field.Clear();
                    break;
                case '\r':
                    break;
                case '\n':
                    EndRecord();
                    line++;
                    recordLine = line;
                    break;
                default:
                    field.Append(c);
                    break;
            }
        }

        if (inQuotes)
            throw new RunSetupException($"A quoted value on line {quoteLine} is never closed.");

        if (fields.Count > 0 || field.Length > 0)
            EndRecord();

        return records;
    }

    private static IReadOnlyList<IReadOnlyList<KeyValue>> ParseJson(string text)
    {
        JsonDocument document;
        try
        {
            document = JsonDocument.Parse(text);
        }
        catch (JsonException ex)
        {
            throw new RunSetupException($"The JSON file is not valid JSON: {ex.Message}");
        }

        using (document)
        {
            if (document.RootElement.ValueKind != JsonValueKind.Array)
                throw new RunSetupException("The JSON file must hold an array with one object per row.");

            var rows = new List<IReadOnlyList<KeyValue>>();
            var index = 0;
            foreach (var element in document.RootElement.EnumerateArray())
            {
                index++;
                if (element.ValueKind != JsonValueKind.Object)
                    throw new RunSetupException($"Row {index} of the JSON file is not an object.");

                var values = element.EnumerateObject()
                    .Select(property => new KeyValue(property.Name, ValueText(property.Value)))
                    .ToList();
                if (values.Count > 0)
                    rows.Add(values);
            }

            return rows;
        }
    }

    // Strings are used as written. Numbers and booleans keep their exact text, null is empty, and nested values are kept as JSON.
    private static string ValueText(JsonElement value) => value.ValueKind switch
    {
        JsonValueKind.String => value.GetString() ?? "",
        JsonValueKind.Null => "",
        _ => value.GetRawText(),
    };
}
