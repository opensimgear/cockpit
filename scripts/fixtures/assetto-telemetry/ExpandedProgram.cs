// Authored SDK-layout pages for opt-in native acceptance. No simulator data or
// hardware outputs. CreateNew refuses to replace another publisher's mappings.
using System;
using System.Diagnostics;
using System.IO;
using System.IO.MemoryMappedFiles;
using System.Text;
using System.Threading;

class ExpandedAssettoTelemetryFixture
{
    static void Int(byte[] page, int offset, int value) { Array.Copy(BitConverter.GetBytes(value), 0, page, offset, 4); }
    static void Float(byte[] page, int offset, float value) { Array.Copy(BitConverter.GetBytes(value), 0, page, offset, 4); }
    static void Text(byte[] page, int offset, string value, bool narrow) { var bytes = (narrow ? Encoding.UTF8 : Encoding.Unicode).GetBytes(value); Array.Copy(bytes, 0, page, offset, bytes.Length); }
    static int Main(string[] args)
    {
        try { Run(args); return 0; }
        catch (Exception error) { if (args.Length == 2) File.WriteAllText(args[1] + ".error", error.ToString()); return 1; }
    }
    static void Run(string[] args)
    {
        if (args.Length != 2 || !Path.IsPathRooted(args[1])) return;
        bool evo = args[0] == "assetto-corsa-evo", acc = args[0] == "assetto-corsa-competizione";
        string prefix = evo ? "Local\\acevo_pmf" : "Local\\acpmf";
        int physicsSize = evo || acc ? 800 : 580, graphicsSize = evo ? 3940 : acc ? 1588 : 296, staticSize = evo ? 208 : acc ? 820 : 684;
        var physics = new byte[physicsSize]; var graphics = new byte[graphicsSize]; var statics = new byte[staticSize];
        Float(physics,4,0.25f); Float(physics,8,0.5f); Float(physics,12,35.5f);
        Int(physics,16,3); Int(physics,20,4500); Float(physics,24,evo ? -0.5f : -0.25f); Float(physics,28,72f);
        Float(physics,32,1.25f); Float(physics,36,-2.5f); Float(physics,40,3.75f);
        Float(physics,44,0f); Float(physics,48,3f); Float(physics,52,4f);
        Float(physics,116,99.25f); Float(physics,192,0.125f); Float(physics,288,21.5f); Float(physics,292,32.5f); Float(physics,308,-0.75f);
        if (evo || acc) {
            Int(physics,588,evo ? 9000 : 1234); // ACC's unused extension must not become its maximum.
            Float(physics,592,-15.5f); Float(physics,716,17.25f);
        }
        Text(statics,0,evo ? "1.0" : acc ? "1.9" : "1.7",evo);
        Text(statics,evo ? 15 : 30,"authored",evo);
        Text(evo ? graphics : statics,evo ? 3086 : 68,"osg_expanded_car",evo);
        Text(statics,evo ? 136 : 134,"osg_expanded_track",evo);
        if (evo) {
            Array.Copy(BitConverter.GetBytes(UInt64.MaxValue),0,graphics,8,8);
            Array.Copy(BitConverter.GetBytes((ushort)4500),0,graphics,40,2);
            graphics[42]=1; Float(graphics,232,91.5f); Text(graphics,3020,"Authored Driver",true);
        } else {
            Int(statics,412,9000); Text(statics,200,"Authored Driver",false); Text(graphics,12,"00:42.123",false);
        }
        using (var p=MemoryMappedFile.CreateNew(prefix+"_physics",physicsSize))
        using (var g=MemoryMappedFile.CreateNew(prefix+"_graphics",graphicsSize))
        using (var s=MemoryMappedFile.CreateNew(prefix+"_static",staticSize))
        using (var pv=p.CreateViewAccessor(0,physicsSize))
        using (var gv=g.CreateViewAccessor(0,graphicsSize))
        using (var sv=s.CreateViewAccessor(0,staticSize))
        {
            sv.WriteArray(0,statics,0,staticSize);
            var watch=Stopwatch.StartNew(); int counter=0; string mode="live",acknowledged="";
            while (watch.Elapsed<TimeSpan.FromMinutes(5) && !File.Exists(args[1])) {
                try { if (File.Exists(args[1]+".mode")) { var requested=File.ReadAllText(args[1]+".mode").Trim(); if(requested=="live"||requested=="paused"||requested=="frozen"||requested=="invalid")mode=requested; } } catch(IOException) {}
                Int(graphics,4,mode=="paused" ? 3 : 2);
                Float(physics,48,mode=="invalid" ? Single.NaN : 3f);
                if(mode!="frozen")counter++;
                Int(physics,0,counter); Int(graphics,0,counter);
                gv.WriteArray(0,graphics,0,graphicsSize); pv.WriteArray(0,physics,0,physicsSize);
                if(acknowledged!=mode) { File.WriteAllText(args[1]+".ack",mode); acknowledged=mode; }
                if(counter==1)File.WriteAllText(args[1]+".ready","ready");
                Thread.Sleep(50);
            }
        }
    }
}
