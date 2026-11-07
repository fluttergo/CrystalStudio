using System.IO;
using System.IO.Compression;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
namespace CrystalStudio;

public sealed record StampDelta(int X,int Y,Cell Before,Cell After,List<Front>? OwnedFronts=null,List<int>? OwnedFrontIndices=null);
public sealed record PlacedStamp(Guid Id,string Name,int X,int Y,List<StampCell> Cells,List<StampDelta> Changes,WallNode? Wall=null)
{
    // Optional ownership metadata; older maps have no generated banks.
    public List<TerrainBankFootprint>? GeneratedBanks { get; init; }
}

public sealed partial class MapDocument
{
    private List<PlacedStamp> placedStamps=[];
    private readonly Dictionary<int,List<PlacedStamp>> stampIndex=[];
    public IReadOnlyList<PlacedStamp> PlacedStamps=>placedStamps;
    public string? MetadataWarning {get;internal set;}
    internal void RestoreStamps(IEnumerable<PlacedStamp> stamps)
    {
        placedStamps=[..stamps];stampIndex.Clear();PickupRevision++;
        foreach(var stamp in placedStamps)IndexStamp(stamp);
    }
    private void IndexStamp(PlacedStamp stamp)
    {
        foreach(var delta in stamp.Changes){int k=(stamp.Y+delta.Y)*Width+stamp.X+delta.X;if(!stampIndex.TryGetValue(k,out var list))stampIndex[k]=list=[];list.Add(stamp);}
    }
    private void AddStamp(PlacedStamp stamp){placedStamps.Add(stamp);IndexStamp(stamp);PickupRevision++;}
    public int RememberIdentifiedStamps(StampScanResult result,EditHistory history)
    {
        if(result.Matches.Count==0)return 0;
        if(placedStamps.Count+result.Matches.Count>100000)throw new InvalidDataException("Too many stamp instances.");
        history.RecordStamps(this);foreach(var stamp in result.Matches)AddStamp(stamp);history.Commit();return result.Matches.Count;
    }
    private void ForgetStamp(PlacedStamp stamp)
    {
        placedStamps.Remove(stamp);PickupRevision++;
        foreach(var delta in stamp.Changes){int k=(stamp.Y+delta.Y)*Width+stamp.X+delta.X;if(stampIndex.TryGetValue(k,out var list)){list.Remove(stamp);if(list.Count==0)stampIndex.Remove(k);}}
    }
    public PlacedStamp? StampAt(int x,int y)
    {
        if(!Contains(x,y)||!stampIndex.TryGetValue(y*Width+x,out var list))return null;
        return list.LastOrDefault(IsStampAvailable);
    }
    internal bool IsStampAvailable(PlacedStamp s)=>s.Changes.Any(c=>c.After.Foreground!=c.Before.Foreground&&OwnsFronts(Get(s.X+c.X,s.Y+c.Y),c))
        ||s.Changes.Any(c=>(c.After.BackImage&0x1FFFFFFF)!=(c.Before.BackImage&0x1FFFFFFF)&&(Get(s.X+c.X,s.Y+c.Y).BackImage&0x1FFFFFFF)==(c.After.BackImage&0x1FFFFFFF))
        ||s.Cells.All(c=>c.Cell.ForegroundImages.Count==0&&(c.Cell.BackImage&0x1FFFFFFF)==0);
    private static bool OwnsFronts(Cell current,StampDelta delta)=>current.Foreground is { } front &&
        (delta.OwnedFronts?.Contains(front) ?? (delta.After.Foreground!=delta.Before.Foreground && delta.After.Foreground==front));
    public bool CanPlaceStamp(IReadOnlyList<StampCell> cells,int x,int y,Guid? moving=null)
    {
        if(moving!=null)return CheckStampPlacement(cells,x,y,moving).Allowed;
        if(cells.Count==0||cells.Count>1000000)return false;
        // Generation tests many overlapping candidates. A yes/no check can stop
        // at the first obstruction without allocating a complete diagnostic report.
        foreach(var s in cells)
        {
            long sx=(long)x+s.X,sy=(long)y+s.Y;
            if(sx<0||sy<0||sx>=Width||sy>=Height)return false;
            var c=Get((int)sx,(int)sy);
            if(c.HasForeground)return false;
        }
        var visited=new HashSet<(int,int)>();
        foreach(var s in cells)if(!visited.Add((s.X,s.Y)))return false;
        return true;
    }
    public StampPlacementResult CheckStampPlacement(IReadOnlyList<StampCell> cells,int x,int y,Guid? moving=null)
    {
        if(cells.Count==0||cells.Count>1000000)return new([],"Stamp must contain between 1 and 1,000,000 cells");
        var blocked=new List<StampBlockedCell>();
        var visited=new HashSet<(long,long)>();
        var original=moving==null?null:placedStamps.FirstOrDefault(s=>s.Id==moving);
        var originalCells=original?.Changes.ToDictionary(c=>(original.X+c.X,original.Y+c.Y));
        foreach(var s in cells)
        {
            long sx=(long)x+s.X,sy=(long)y+s.Y;
            if(!visited.Add((sx,sy)))return new(blocked,"Stamp contains duplicate cell coordinates");
            if(sx<0||sy<0||sx>=Width||sy>=Height){blocked.Add(new(sx,sy,StampBlockReason.MapBoundary));continue;}
            var c=Get((int)sx,(int)sy);
            if(originalCells?.TryGetValue(((int)sx,(int)sy),out var delta)==true)c=RemoveContribution(c,delta);
            if(c.HasForeground)blocked.Add(new(sx,sy,StampBlockReason.OccupiedObject));
        }
        return new(blocked);
    }
    private static Cell MergeStamp(Cell before,Cell a)
    {
        var c=before.Copy();
        if(a.BackImage!=0){if((a.BackImage&0x1FFFFFFF)!=0){c.BackLibrary=a.BackLibrary;c.BackImage=(c.BackImage&0x60000000)|a.BackImage;}else c.BackImage|=a.BackImage;}
        if(a.MiddleImage!=0){c.MiddleLibrary=a.MiddleLibrary;c.MiddleImage=a.MiddleImage;}
        if (a.Foreground is { } front) c.Foreground = front;
        if(a.Door!=0){c.Door=a.Door;c.DoorOffset=a.DoorOffset;}
        if(a.Light!=0)c.Light=a.Light;
        if(a.TileFrames!=0){c.TileFrames=a.TileFrames;c.TileAnimation=a.TileAnimation;c.TileOffset=a.TileOffset;}
        if(a.MiddleFrames!=0){c.MiddleFrames=a.MiddleFrames;c.MiddleTick=a.MiddleTick;}
        return c;
    }
    // Restore only values still owned by this instance; keep later edits to other layers.
    private static Cell RemoveContribution(Cell current,StampDelta d)
    {
        var c=current.Copy();var b=d.Before;var a=d.After;
        if(((a.BackImage&0x1FFFFFFF)!=(b.BackImage&0x1FFFFFFF)||a.BackLibrary!=b.BackLibrary)&&(c.BackImage&0x1FFFFFFF)==(a.BackImage&0x1FFFFFFF)&&c.BackLibrary==a.BackLibrary){c.BackLibrary=b.BackLibrary;c.BackImage=(c.BackImage&0x60000000)|(b.BackImage&0x1FFFFFFF);}
        c.BackImage&=~((a.BackImage&~b.BackImage)&0x60000000);
        if((a.MiddleImage!=b.MiddleImage||a.MiddleLibrary!=b.MiddleLibrary)&&c.MiddleImage==a.MiddleImage&&c.MiddleLibrary==a.MiddleLibrary){c.MiddleImage=b.MiddleImage;c.MiddleLibrary=b.MiddleLibrary;}
        if (c.Foreground is { } front && (d.OwnedFronts?.Contains(front) ?? (a.Foreground != b.Foreground && a.Foreground == front)))
            c.Foreground = b.Foreground;
        if((a.Door!=b.Door||a.DoorOffset!=b.DoorOffset)&&c.Door==a.Door&&c.DoorOffset==a.DoorOffset){c.Door=b.Door;c.DoorOffset=b.DoorOffset;}
        if(a.Light!=b.Light&&c.Light==a.Light)c.Light=b.Light;
        if((a.TileFrames!=b.TileFrames||a.TileAnimation!=b.TileAnimation||a.TileOffset!=b.TileOffset)&&c.TileFrames==a.TileFrames&&c.TileAnimation==a.TileAnimation&&c.TileOffset==a.TileOffset){c.TileFrames=b.TileFrames;c.TileAnimation=b.TileAnimation;c.TileOffset=b.TileOffset;}
        if((a.MiddleFrames!=b.MiddleFrames||a.MiddleTick!=b.MiddleTick)&&c.MiddleFrames==a.MiddleFrames&&c.MiddleTick==a.MiddleTick){c.MiddleFrames=b.MiddleFrames;c.MiddleTick=b.MiddleTick;}
        return c;
    }
    public PlacedStamp? PlaceStamp(IReadOnlyList<StampCell> cells,int x,int y,string name,EditHistory history,Action<int,int> invalidate,Guid? id=null)
    {
        if(!CanPlaceStamp(cells,x,y)||placedStamps.Count>=100000||name.Length>4096)return null;
        var source=cells.Select(s=>new StampCell(s.X,s.Y,s.Cell.Copy())).ToList();
        history.RecordStamps(this);var deltas=new List<StampDelta>();
        foreach(var s in source){int sx=x+s.X,sy=y+s.Y;history.Record(this,sx,sy);var before=Get(sx,sy).Copy();var after=MergeStamp(before,s.Cell);Set(sx,sy,after);deltas.Add(new(s.X,s.Y,before,after.Copy()));invalidate(sx,sy);}
        var stamp=new PlacedStamp(id??Guid.NewGuid(),name,x,y,source,deltas);AddStamp(stamp);return stamp;
    }
    public bool DeleteStamp(Guid id,EditHistory history,Action<int,int> invalidate)
    {
        var stamp=placedStamps.FirstOrDefault(s=>s.Id==id);if(stamp==null)return false;
        history.RecordStamps(this);
        foreach(var delta in stamp.Changes){int sx=stamp.X+delta.X,sy=stamp.Y+delta.Y;history.Record(this,sx,sy);var restored=RemoveContribution(Get(sx,sy),delta);
            if(stampIndex.TryGetValue(sy*Width+sx,out var owners))foreach(var other in owners.Where(o=>o.Id!=id))foreach(var change in other.Changes.Where(c=>other.X+c.X==sx&&other.Y+c.Y==sy))restored.BackImage|=(change.After.BackImage&~change.Before.BackImage)&0x60000000;
            Set(sx,sy,restored);invalidate(sx,sy);}
        ForgetStamp(stamp);return true;
    }
    public bool MoveStamp(Guid id,int x,int y,EditHistory history,Action<int,int> invalidate)
    {
        var stamp=placedStamps.FirstOrDefault(s=>s.Id==id);if(stamp==null||!CanPlaceStamp(stamp.Cells,x,y,id))return false;
        if(x==stamp.X&&y==stamp.Y)return true;
        DeleteStamp(id,history,invalidate);return PlaceStamp(stamp.Cells,x,y,stamp.Name,history,invalidate,id)!=null;
    }
}

internal static class StampMetadata
{
    private static readonly byte[] magic=Encoding.ASCII.GetBytes("CRYSTAL-STAMPS01");
    // Large coasts own many ordinary water/collision cells. Their zero-valued
    // fields need not be repeated three times in source/before/after metadata.
    // Missing fields already deserialize to these defaults in existing readers.
    private static readonly JsonSerializerOptions compact = new()
        { DefaultIgnoreCondition = System.Text.Json.Serialization.JsonIgnoreCondition.WhenWritingDefault };
    internal sealed record Payload(int Version,List<PlacedStamp> Stamps);
    private const int Limit=256*1024*1024;
    private sealed class LimitedWriteStream(Stream target):Stream
    {
        private long written;
        public override bool CanRead=>false;public override bool CanSeek=>false;public override bool CanWrite=>true;
        public override long Length=>written;public override long Position{get=>written;set=>throw new NotSupportedException();}
        public override void Write(byte[] buffer,int offset,int count)=>Write(buffer.AsSpan(offset,count));
        public override void Write(ReadOnlySpan<byte> buffer){if(written+buffer.Length>Limit)throw new InvalidDataException("Placed-object metadata is too large.");target.Write(buffer);written+=buffer.Length;}
        public override void Flush()=>target.Flush();public override int Read(byte[] b,int o,int c)=>throw new NotSupportedException();
        public override long Seek(long o,SeekOrigin s)=>throw new NotSupportedException();public override void SetLength(long v)=>throw new NotSupportedException();
    }
    public static void Write(BinaryWriter w,MapDocument d)
    {
        if(d.PlacedStamps.Count==0)return;
        int version=d.PlacedStamps.Any(s=>s.Wall!=null)?3:d.PlacedStamps.Any(s=>s.Changes.Any(c=>c.OwnedFronts!=null))?2:1;
        using var compressed=new MemoryStream();using(var zip=new BrotliStream(compressed,CompressionLevel.Fastest,true))StudioJson.Serialize(new LimitedWriteStream(zip),new Payload(version,[..d.PlacedStamps]),compact);
        if(compressed.Length>Limit)throw new InvalidDataException("Placed-object metadata is too large.");
        var data=compressed.ToArray();w.Write(data);w.Write((long)data.Length);w.Write(SHA256.HashData(data));w.Write(magic);
    }
    /// <summary>
    /// Classic .map files cannot carry the trailing stamp metadata block, so Save stores the
    /// full Crystal copy in a sidecar. Try that when the main file has no metadata of its own.
    /// </summary>
    public static void ReadWithSidecar(string path, MapDocument d)
    {
        Read(path, d);
        if (d.MetadataWarning == null && d.PlacedStamps.Count > 0) return;
        string side = MapDocument.ClassicSidecar(path);
        if (!File.Exists(side)) return;
        try { Read(side, d); }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or InvalidDataException) { }
    }
    public static void Read(string path,MapDocument d)
    {
        using var r=new BinaryReader(File.OpenRead(path));if(r.BaseStream.Length<56)return;
        r.BaseStream.Position=r.BaseStream.Length-16;if(!r.ReadBytes(16).SequenceEqual(magic))return;
        try
        {
            r.BaseStream.Position=r.BaseStream.Length-56;long length=r.ReadInt64();var hash=r.ReadBytes(32);
            if(length<=0||length>Limit||length>r.BaseStream.Length-64)throw new InvalidDataException("Invalid metadata size.");
            r.BaseStream.Position=0;int mapVersion=r.ReadInt16();
            long mapEnd=mapVersion==1 ? 8+(long)d.Width*d.Height*26 : 8+(long)d.Width*d.Height*33+d.Cells.Sum(c=>7L*c.Value.ForegroundImages.Count);
            if(r.BaseStream.Length-56-length!=mapEnd)throw new InvalidDataException("Metadata is not positioned after the map cells.");
            r.BaseStream.Position=r.BaseStream.Length-56-length;var data=r.ReadBytes((int)length);
            if(!SHA256.HashData(data).SequenceEqual(hash))throw new InvalidDataException("Metadata checksum failed.");
            using var zip=new BrotliStream(new MemoryStream(data),CompressionMode.Decompress);using var expanded=new MemoryStream();byte[] buffer=new byte[8192];int n;
            while((n=zip.Read(buffer))>0){if(expanded.Length+n>Limit)throw new InvalidDataException("Metadata expands beyond the size limit.");expanded.Write(buffer,0,n);}
            var payload=StudioJson.Deserialize<Payload>(expanded.ToArray())??throw new InvalidDataException("Empty stamp metadata.");
            if(payload.Version is not (1 or 2 or 3)||payload.Stamps==null||payload.Stamps.Count>100000||payload.Stamps.Select(s=>s.Id).Distinct().Count()!=payload.Stamps.Count)throw new InvalidDataException("Invalid stamp metadata version or instances.");
            foreach(var s in payload.Stamps)
            {
                if(s.Cells==null||s.Changes==null||s.Cells.Count==0||s.Cells.Count>1000000||s.Cells.Count!=s.Changes.Count||s.Name==null||s.Name.Length>4096)throw new InvalidDataException("Invalid stamp instance.");
                if (s.GeneratedBanks is {} banks)
                {
                    var owned = s.Changes.Where(c => c.OwnedFronts?.Count == 1 && c.OwnedFrontIndices is [0])
                        .Select(c => (c.X, c.Y)).ToHashSet();
                    if (banks.Count != s.Changes.Count || banks.Count == 0 || banks.Select(b => (b.X,b.Y)).Distinct().Count() != banks.Count ||
                        banks.Any(b => b.Height is < 1 or > 544 || !owned.Contains((b.X,b.Y))))
                        throw new InvalidDataException("Invalid generated bank ownership.");
                }
                if(s.Wall is { } wall && (payload.Version < 3 || string.IsNullOrWhiteSpace(wall.FamilyId) || wall.FamilyId.Length > 128 || wall.Mask is < 0 or > 15 || wall.Corners is < 0 or > 15 || wall.VariationSeed < 0 || wall.LayoutRevision < 0))throw new InvalidDataException("Invalid wall connection metadata.");
                for(int i=0;i<s.Cells.Count;i++){var c=s.Cells[i];var a=s.Changes[i];if(c.X!=a.X||c.Y!=a.Y||c.Cell==null||a.Before==null||a.After==null||(long)s.X+c.X<0||(long)s.Y+c.Y<0||(long)s.X+c.X>=d.Width||(long)s.Y+c.Y>=d.Height)throw new InvalidDataException("Stamp instance outside the map or malformed.");}
                if(s.Cells.Select(c=>(c.X,c.Y)).Distinct().Count()!=s.Cells.Count)throw new InvalidDataException("Duplicate stamp cells.");
                foreach(var change in s.Changes)
                {
                    if(change.OwnedFronts?.Count>1)throw new InvalidDataException("Invalid foreground ownership.");
                    if(change.OwnedFrontIndices is List<int> indices&&(change.OwnedFronts==null||indices.Count!=change.OwnedFronts.Count||indices.Distinct().Count()!=indices.Count||indices.Any(i=>i<0||i>=change.After.ForegroundImages.Count)||indices.Where((index,i)=>change.After.ForegroundImages[index]!=change.OwnedFronts[i]).Any()))throw new InvalidDataException("Invalid foreground ownership indices.");
                }
            }
            d.RestoreStamps(payload.Stamps);
        }
        catch(Exception ex) when(ex is IOException or InvalidDataException or JsonException or ArgumentException or NullReferenceException)
        {d.MetadataWarning="Placed-object metadata could not be read; map tiles are intact. "+ex.Message;}
    }
}
