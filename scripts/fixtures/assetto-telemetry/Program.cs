// Authored test pages, never a copy of simulator telemetry. Run only from the
// opt-in assetto-native acceptance, using private executable and stop paths.
using System;
using System.Diagnostics;
using System.IO;
using System.IO.MemoryMappedFiles;
using System.Text;
using System.Threading;

class AssettoTelemetryFixture
{
    static void Int(byte[] page, int offset, int value) { Array.Copy(BitConverter.GetBytes(value), 0, page, offset, 4); }
    static void Text(byte[] page, int offset, string value, bool narrow) { var bytes = (narrow ? Encoding.UTF8 : Encoding.Unicode).GetBytes(value); Array.Copy(bytes, 0, page, offset, bytes.Length); }
    static int Main(string[] args)
    {
        try { Run(args); return 0; }
        catch (Exception error) { if (args.Length == 2) File.WriteAllText(args[1] + ".error", error.ToString()); return 1; }
    }
    static void Run(string[] args)
    {
        if (args.Length != 2 || !Path.IsPathRooted(args[1])) return;
        bool evo = args[0] == "assetto-corsa-evo";
        string prefix = evo ? "Local\\acevo_pmf" : "Local\\acpmf";
        var physics = new byte[4096]; var graphics = new byte[4096]; var statics = new byte[4096];
        Int(physics, 16, 3); Int(physics, 20, 4500);
        Array.Copy(BitConverter.GetBytes(123.5f), 0, physics, 28, 4);
        Int(physics, 588, evo ? 9000 : 1234); // AC/ACC must ignore this offset.
        Int(graphics, 4, 2);
        Text(statics, 0, "1.0", evo); Text(statics, evo ? 15 : 30, "fixture", evo);
        Text(evo ? graphics : statics, evo ? 3086 : 68, "osg_fixture_car", evo);
        Text(statics, evo ? 136 : 134, "osg_fixture_track", evo);
        if (!evo) Int(statics, 412, 9000);
        // Only create new mappings. A retained mapping from another application
        // must make this authored publisher fail without overwriting its pages.
        int physicsSize = evo ? 592 : 32, graphicsSize = evo ? 3119 : 8, staticSize = evo ? 169 : 416;
        using (var p = MemoryMappedFile.CreateNew(prefix + "_physics", 4096))
        using (var g = MemoryMappedFile.CreateNew(prefix + "_graphics", 4096))
        using (var s = MemoryMappedFile.CreateNew(prefix + "_static", 4096))
        using (var pv = p.CreateViewAccessor(0, physicsSize))
        using (var gv = g.CreateViewAccessor(0, graphicsSize))
        using (var sv = s.CreateViewAccessor(0, staticSize))
        {
            gv.WriteArray(0, graphics, 0, graphicsSize); sv.WriteArray(0, statics, 0, staticSize);
            Int(physics, 0, 1); pv.WriteArray(0, physics, 0, physicsSize);
            File.WriteAllText(args[1] + ".ready", "ready");
            var watch = Stopwatch.StartNew(); int counter = 0;
            while (watch.Elapsed < TimeSpan.FromMinutes(3) && !File.Exists(args[1]))
            {
                Int(physics, 0, ++counter); pv.WriteArray(0, physics, 0, physicsSize);
                Thread.Sleep(50);
            }
        }
    }
}
