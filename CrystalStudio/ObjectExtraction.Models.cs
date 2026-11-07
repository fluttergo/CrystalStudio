using System.IO;
using System.Security.Cryptography;

namespace CrystalStudio;

internal enum ExtractionKind { Metadata, Template, Discovered }
internal sealed record ExtractionOccurrence(string Source, int X, int Y, bool CrossesSelection);
internal sealed record ExtractionCandidate(Guid Id, string Name, string Category, string Reason, ExtractionKind Kind,
    List<StampCell> Cells, List<StampCell> SuggestedCollisions, List<ExtractionOccurrence> Occurrences,
    List<StampCell> Context, string? TemplatePath = null, bool Reviewed = false, bool IncludeBoundary = false, Rect? ContextArea = null)
{
    public string Fingerprint => ExtractionContent.Fingerprint(Cells);
    public bool CanExport => Reviewed && Cells.Any(c => c.Cell.HasForeground) &&
        (IncludeBoundary || Occurrences.Any(o => !o.CrossesSelection));
    public string DisplayName => Name;
    public string Label => Name;
    public string Caption
    {
        get
        {
            if(TemplatePath!=null)foreach(string separator in new[]{" — "," – "," - "})
            {
                string suffix=separator+Path.GetFileNameWithoutExtension(TemplatePath);
                if(Name.EndsWith(suffix,StringComparison.OrdinalIgnoreCase))return LocalText.T(Name[..^suffix.Length].Trim());
            }
            return LocalText.T(Name);
        }
    }
    public string Description => LocalText.Format("{0} · {1} fragments · {2} sources", LocalText.T(Category), Cells.Count(c=>c.Cell.HasForeground), Occurrences.Count)
        + "\n" + string.Join("\n",Reason.Split('\n').Select(LocalText.T)) + "\n" + LocalText.T(Reviewed ? "Reviewed" : "Unreviewed");
}

internal static class ExtractionContent
{
    internal const int CollisionMask = 0x60000000;
    public static List<StampCell> Copy(IEnumerable<StampCell> cells) => cells.Select(c=>new StampCell(c.X,c.Y,c.Cell.Copy())).ToList();
    public static List<StampCell> Normalize(IEnumerable<StampCell> cells)
    {
        var list=Copy(cells.Where(c=>!c.Cell.Empty));
        if(list.Select(c=>(c.X,c.Y)).Distinct().Count()!=list.Count) throw new InvalidDataException("Repeated object coordinates.");
        foreach(var c in list)
        {
            if((c.Cell.BackImage&0x1FFFFFFF)==0)c.Cell.BackLibrary=0;
            if(c.Cell.MiddleImage==0)c.Cell.MiddleLibrary=0;
        }
        return CellCodec.CropStamp(list).OrderBy(c=>c.Y).ThenBy(c=>c.X).ToList();
    }
    public static string Fingerprint(IEnumerable<StampCell> cells)
    {
        var normalized=Normalize(cells);
        using var stream=new MemoryStream();using(var writer=new BinaryWriter(stream,System.Text.Encoding.UTF8,true))
        {
            writer.Write(2);writer.Write(normalized.Count);
            foreach(var cell in normalized){writer.Write(cell.X);writer.Write(cell.Y);CellCodec.Write(writer,cell.Cell,false);}
        }
        return Convert.ToHexStringLower(SHA256.HashData(stream.GetBuffer().AsSpan(0,(int)stream.Length)));
    }
    public static MapDocument Snapshot(MapDocument source)
    {
        var result=new MapDocument(source.Width,source.Height);
        foreach(var pair in source.Cells)result.Set(pair.Key%source.Width,pair.Key/source.Width,pair.Value.Copy());
        result.RestoreStamps(source.PlacedStamps.Select(s=>s with
        {
            Cells=Copy(s.Cells), Changes=s.Changes.Select(d=>d with
            {Before=d.Before.Copy(),After=d.After.Copy(),OwnedFronts=d.OwnedFronts?.ToList(),OwnedFrontIndices=d.OwnedFrontIndices?.ToList()}).ToList()
        }));
        return result;
    }
}

// Candidates are value snapshots. Every review command replaces records, so undo never
// holds the editor's document or its history. Cell copies are made at mutation boundaries.
internal sealed class ExtractionReview
{
    public List<ExtractionCandidate> Candidates { get; private set; }
    private readonly Stack<List<ExtractionCandidate>> undo=[],redo=[];
    public bool CanUndo=>undo.Count>0;
    public bool CanRedo=>redo.Count>0;
    public ExtractionReview(IEnumerable<ExtractionCandidate> candidates)=>Candidates=candidates.ToList();
    private void Change(List<ExtractionCandidate> value)
    {
        undo.Push(Candidates);redo.Clear();Candidates=value;
        if(undo.Count>40){var keep=undo.Take(40).Reverse().ToArray();undo.Clear();foreach(var item in keep)undo.Push(item);}
    }
    public void Undo(){if(undo.TryPop(out var value)){redo.Push(Candidates);Candidates=value;}}
    public void Redo(){if(redo.TryPop(out var value)){undo.Push(Candidates);Candidates=value;}}
    public void ApplyAi(IReadOnlyDictionary<Guid,AiObjectRecognition> results)
    {
        if(results.Count==0)return;
        if(results.Keys.Any(id=>!Candidates.Any(c=>c.Id==id)) || results.Values.Any(r=>
            string.IsNullOrWhiteSpace(r.Name)||string.IsNullOrWhiteSpace(r.Category)||
            string.IsNullOrWhiteSpace(r.Reason)||!Enum.IsDefined(r.Completeness)))
            throw new InvalidDataException("Invalid AI recognition results.");
        // Removal and metadata updates share one history entry; undo restores every original candidate.
        Change(Candidates.Where(c=>!results.TryGetValue(c.Id,out var result)||result.Completeness!=AiObjectCompleteness.Incomplete)
            .Select(c=>results.TryGetValue(c.Id,out var result)?c with
        {
            Name=result.Name,Category=result.Category,Reviewed=false,
            Reason=c.Reason+"\n"+LocalText.T("AI recognition")+": "+
                LocalText.T(result.Completeness==AiObjectCompleteness.Complete?"Complete object":"Completeness uncertain")+" · "+result.Reason
        }:c).ToList());
    }
    public void Update(IEnumerable<Guid> ids,string? name=null,string? category=null,bool? reviewed=null,bool? boundary=null)
    {
        var selected=ids.ToHashSet();
        if(name!=null&&string.IsNullOrWhiteSpace(name))throw new InvalidDataException("Enter an object name.");
        Change(Candidates.Select(c=>selected.Contains(c.Id)?c with
        {Name=name??c.Name,Category=string.IsNullOrWhiteSpace(category)?c.Category:category.Trim(),Reviewed=reviewed??c.Reviewed,IncludeBoundary=boundary??c.IncludeBoundary}:c).ToList());
    }
    public void Exclude(IEnumerable<Guid> ids)
    {var selected=ids.ToHashSet();Change(Candidates.Where(c=>!selected.Contains(c.Id)).ToList());}
    public void RemoveFragments(Guid id,IEnumerable<(int X,int Y)> points,bool split)
    {
        var selected=points.ToHashSet();var candidate=Candidates.Single(c=>c.Id==id);
        var cells=ExtractionContent.Copy(candidate.Cells);var part=new List<StampCell>();
        foreach(var cell in cells.Where(c=>selected.Contains((c.X,c.Y))&&c.Cell.HasForeground))
        {
            // Collision at an owned anchor follows a separated fragment.
            part.Add(new(cell.X,cell.Y,cell.Cell.Copy()));cell.Cell.Foreground=null;cell.Cell.BackImage=0;cell.Cell.Door=cell.Cell.DoorOffset=cell.Cell.Light=0;
        }
        if(part.Count==0)throw new InvalidDataException("Select foreground fragments first.");
        var next=Candidates.Where(c=>c.Id!=id).ToList();
        bool hasRemaining=cells.Any(c=>c.Cell.HasForeground);
        if(hasRemaining)next.Add(candidate with {Cells=cells.Where(c=>!c.Cell.Empty).ToList(),Reviewed=false,TemplatePath=null});
        else if(split)part.AddRange(cells.Where(c=>!c.Cell.Empty));
        if(split)next.Add(candidate with {Id=Guid.NewGuid(),Name=candidate.Name+" (2)",Cells=part,Reviewed=false,TemplatePath=null});
        Change(next);
    }
    public void Collision(Guid id,int x,int y,int bits)
    {
        bits&=ExtractionContent.CollisionMask;var c=Candidates.Single(c=>c.Id==id);
        var cells=ExtractionContent.Copy(c.Cells);var cell=cells.FirstOrDefault(s=>s.X==x&&s.Y==y);
        int allowed=(cell?.Cell.BackImage??0)&ExtractionContent.CollisionMask;
        allowed|=c.SuggestedCollisions.Where(s=>s.X==x&&s.Y==y).Aggregate(0,(b,s)=>b|s.Cell.BackImage);
        if((bits&~allowed)!=0)throw new InvalidDataException("Only collision present in the source can be included.");
        if(cell==null){cell=new(x,y,new());cells.Add(cell);}
        cell.Cell.BackImage=(cell.Cell.BackImage&~ExtractionContent.CollisionMask)|bits;
        Change(Candidates.Select(a=>a.Id==id?a with {Cells=cells.Where(s=>!s.Cell.Empty).ToList(),Reviewed=false,TemplatePath=null}:a).ToList());
    }
    public void Merge(IEnumerable<Guid> ids)
    {
        var selected=ids.ToHashSet();var items=Candidates.Where(c=>selected.Contains(c.Id)).ToList();
        if(items.Count<2)throw new InvalidDataException("Select at least two candidates.");
        var first=items[0];var origin=first.Occurrences[0];
        var offsets=new Dictionary<Guid,(int X,int Y)>{{first.Id,(0,0)}};
        foreach(var item in items.Skip(1))
        {
            var nearest=item.Occurrences.Where(o=>o.Source==origin.Source)
                .OrderBy(o=>Math.Abs((long)o.X-origin.X)+Math.Abs((long)o.Y-origin.Y)).FirstOrDefault();
            if(nearest==null)throw new InvalidDataException("Merge requires candidates with occurrences in the same source.");
            offsets[item.Id]=(nearest.X-origin.X,nearest.Y-origin.Y);
        }
        var positions=items.ToDictionary(c=>c.Id,c=>c.Occurrences.DistinctBy(o=>(o.Source,o.X,o.Y)).ToDictionary(o=>(o.Source,o.X,o.Y)));
        var mergedOccurrences=new List<ExtractionOccurrence>();var consumed=items.ToDictionary(c=>c.Id,_=>new HashSet<ExtractionOccurrence>());
        foreach(var point in first.Occurrences)
        {
            var matches=new List<(Guid Id,ExtractionOccurrence Point)>();
            foreach(var item in items)
            {
                var offset=offsets[item.Id];
                if(!positions[item.Id].TryGetValue((point.Source,point.X+offset.X,point.Y+offset.Y),out var match))break;
                matches.Add((item.Id,match));
            }
            if(matches.Count!=items.Count)continue;
            mergedOccurrences.Add(point with{CrossesSelection=matches.Any(m=>m.Point.CrossesSelection)});
            foreach(var match in matches)consumed[match.Id].Add(match.Point);
        }
        var cells=new Dictionary<(int,int),Cell>();var suggestions=new Dictionary<(int,int),Cell>();
        foreach(var c in items)
        {
            var (dx,dy)=offsets[c.Id];
            foreach(var s in c.Cells)
            {
                var key=(s.X+dx,s.Y+dy);
                if(!cells.TryGetValue(key,out var cell))cells[key]=cell=new();
                if(cell.HasForeground&&s.Cell.HasForeground)throw new InvalidDataException("Merged objects overlap at a foreground anchor.");
                if(s.Cell.HasForeground){cell.Foreground=s.Cell.Foreground;cell.Door=s.Cell.Door;cell.DoorOffset=s.Cell.DoorOffset;cell.Light=s.Cell.Light;}
                cell.BackImage|=s.Cell.BackImage&ExtractionContent.CollisionMask;
            }
            foreach(var s in c.SuggestedCollisions){var key=(s.X+dx,s.Y+dy);if(!suggestions.TryGetValue(key,out var v))suggestions[key]=v=new();v.BackImage|=s.Cell.BackImage;}
        }
        var context=new Dictionary<(int,int),Cell>();
        foreach(var c in items)
        {
            var offset=offsets[c.Id];var representative=c.Occurrences[0];
            if(representative.Source!=origin.Source||representative.X!=origin.X+offset.X||representative.Y!=origin.Y+offset.Y)continue;
            foreach(var s in c.Context)context.TryAdd((s.X+offset.X,s.Y+offset.Y),s.Cell.Copy());
        }
        foreach(var pair in cells)
            if(!context.ContainsKey(pair.Key))context[pair.Key]=pair.Value.Copy();
        var merged=first with {Id=Guid.NewGuid(),Cells=cells.Select(p=>new StampCell(p.Key.Item1,p.Key.Item2,p.Value)).ToList(),
            SuggestedCollisions=suggestions.Select(p=>new StampCell(p.Key.Item1,p.Key.Item2,p.Value)).ToList(),
            Context=context.Select(p=>new StampCell(p.Key.Item1,p.Key.Item2,p.Value)).ToList(),
            Occurrences=mergedOccurrences,Reviewed=false,TemplatePath=null,
            ContextArea=null,Reason="Manually merged; review fragments and collision."};
        var remaining=Candidates.Where(c=>!selected.Contains(c.Id)).ToList();
        // Instances without matching neighbouring parts remain available as their
        // original candidates. A review command must never discard source records.
        foreach(var c in items)
        {
            var rest=c.Occurrences.Where(o=>!consumed[c.Id].Contains(o)).ToList();
            if(rest.Count>0)remaining.Add(c with{Occurrences=rest});
        }
        Change(remaining.Append(merged).ToList());
    }
}

// Shared clipboard format for carrying an ExtractionCandidate from the extraction window
// onto the main map editor (drop handled in MainWindow.SetCompositions). A single source
// of truth keeps the drag source and drop target in sync.
internal static class ExtractionDrag
{
    public const string Candidate = "application/x-crystal-extraction-candidate";
}
