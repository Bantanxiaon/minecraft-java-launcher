using System.Text;
using System.Text.Json;
using SHLauncher.Contracts.Downloads;
namespace SHLauncher.Downloads;
public sealed class JsonDownloadEventSink(string directory,long maxFileBytes=32*1024*1024):IDownloadEventSink,IDisposable
{
    private readonly object _gate=new();
    private readonly Dictionary<Guid,Slot> _writers=new();
    private readonly Queue<Guid> _order=new();
    private readonly long _limit=maxFileBytes>=1024?maxFileBytes:throw new ArgumentOutOfRangeException(nameof(maxFileBytes));
    public void Write(DownloadEvent entry)
    {
        lock(_gate)
        {
            if(!_writers.TryGetValue(entry.SessionId,out var slot))
            {
                Directory.CreateDirectory(directory);
                if(_writers.Count>=8){var oldest=_order.Dequeue();_writers[oldest].Writer.Dispose();_writers.Remove(oldest);}
                var index=Latest(entry.SessionId).FirstOrDefault()?.Index??0;
                slot=new(index,Open(entry.SessionId,index));_writers.Add(entry.SessionId,slot);_order.Enqueue(entry.SessionId);
            }
            var line=JsonSerializer.Serialize(entry);
            if(slot.Writer.BaseStream.Length>0 && slot.Writer.BaseStream.Length+Encoding.UTF8.GetByteCount(line)+2>_limit)
            {slot.Writer.Dispose();slot.Index++;slot.Writer=Open(entry.SessionId,slot.Index);}
            slot.Writer.WriteLine(line);
        }
    }
    private StreamWriter Open(Guid id,int index)=>new(new FileStream(Path.Combine(directory,id.ToString("N")+"."+index.ToString("D6")+".jsonl"),
        FileMode.Append,FileAccess.Write,FileShare.Read),new UTF8Encoding(false)){AutoFlush=true};
    private IReadOnlyList<LogPart> Latest(Guid id)
    {
        if(!Directory.Exists(directory))return [];
        return Directory.EnumerateFiles(directory,id.ToString("N")+"*.jsonl").Select(path=>{
            var name=Path.GetFileNameWithoutExtension(path);var dot=name.LastIndexOf('.');
            return new LogPart(path,dot>=0&&int.TryParse(name[(dot+1)..],out var number)?number:-1);
        }).OrderByDescending(p=>p.Index).Take(2).ToArray();
    }
    public IReadOnlyList<DownloadEvent> ReadTail(Guid id)
    {
        lock(_gate)
        {
            try
            {
                var entries=new Queue<DownloadEvent>();
                foreach(var part in Latest(id).Reverse())
                {
                    using var stream=new FileStream(part.Path,FileMode.Open,FileAccess.Read,FileShare.ReadWrite);
                    var offset=Math.Max(0,stream.Length-64*1024);stream.Seek(offset,SeekOrigin.Begin);
                    using var reader=new StreamReader(stream,Encoding.UTF8);if(offset>0)reader.ReadLine();
                    while(reader.ReadLine() is { } line)
                    {
                        try{if(JsonSerializer.Deserialize<DownloadEvent>(line) is { } entry&&entry.SessionId==id){entries.Enqueue(entry);if(entries.Count>40)entries.Dequeue();}}
                        catch(JsonException){ /* Interrupted final line is not a complete event. */ }
                    }
                }
                return entries.ToArray();
            }
            catch(Exception error)when(error is IOException or UnauthorizedAccessException){return [];}
        }
    }
    public void Dispose(){lock(_gate){foreach(var slot in _writers.Values)slot.Writer.Dispose();_writers.Clear();_order.Clear();}}
    private sealed class Slot(int index,StreamWriter writer){internal int Index=index;internal StreamWriter Writer=writer;}
    private sealed record LogPart(string Path,int Index);
}
