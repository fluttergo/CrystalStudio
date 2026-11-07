using System.IO;

namespace CrystalStudio;

/// <summary>
/// Alternative input path for the extraction tool: instead of scanning maps, read the
/// already-classified standard object catalogue (the very same palette.json that fills
/// the main window's Categories panel) directly into reviewable candidates.
///
/// No map input is required and no new .X files are produced when saving: every
/// candidate already has a content fingerprint present in the catalogue, so
/// ObjectExtractionExport.Save reuses the existing entry and only updates its
/// name / type / reviewStatus. Previews still use the configured resource folder.
/// </summary>
internal static class ObjectExtractionCatalogue
{
    /// <summary>All classification values already used by the catalogue, for the
    /// category auto-complete in the candidate detail panel.</summary>
    internal static string[] Categories(string indexPath)
    {
        var index = StampCategories.Read(indexPath);
        return index.Objects
            .SelectMany(stamp => new[] { stamp.Type, stamp.Subtype, stamp.Direction }
                .Concat(stamp.Regions).Concat(stamp.RegionGroups)
                .Concat(stamp.Environments).Concat(stamp.Tags))
            .Where(value => !string.IsNullOrWhiteSpace(value))
            .Select(value => value!.Trim())
            .Distinct(StringComparer.OrdinalIgnoreCase)
            .OrderBy(value => value, StringComparer.OrdinalIgnoreCase)
            .ToArray();
    }

    internal static ExtractionSession Load(string indexPath, CancellationToken token,
        IProgress<ExtractionProgress>? progress = null)
    {
        var index = StampCategories.Read(indexPath);
        var candidates = new List<ExtractionCandidate>();
        var warnings = new List<string>();
        int read = 0;
        foreach (var stamp in index.Objects)
        {
            token.ThrowIfCancellationRequested();
            read++;
            if (read % 500 == 0)
                progress?.Report(new("Reading existing categories", 0, candidates.Count, candidates.Count, stamp.File));
            string path;
            List<StampCell> cells;
            try
            {
                path = StampCategories.Resolve(index.ObjectRoot, stamp.File);
                cells = ExtractionContent.Normalize(CellCodec.LoadStamp(path, token));
            }
            catch (Exception ex) when (ex is IOException or InvalidDataException
                                       or UnauthorizedAccessException or ArgumentException)
            { warnings.Add(stamp.File + ": " + ex.Message); continue; }
            // Keep every classified entry so the category list matches the main
            // Categories panel. Entries without foreground artwork get an empty
            // preview and CanExport == false; they are never written by Save.
            string category = string.IsNullOrWhiteSpace(stamp.Type) ? "Unclassified" : stamp.Type!.Trim();
            candidates.Add(new ExtractionCandidate(
                Guid.NewGuid(), stamp.Name, category,
                LocalText.Format("Loaded from the existing object catalogue: {0}", stamp.File),
                ExtractionKind.Template, cells, [],
                [new ExtractionOccurrence(stamp.File, 0, 0, false)], [],
                TemplatePath: path, Reviewed: stamp.ReviewStatus == "reviewed"));
        }
        return new ExtractionSession("existing-categories-" + Guid.NewGuid().ToString("N"),
            candidates, new List<string> { indexPath }, warnings);
    }
}
