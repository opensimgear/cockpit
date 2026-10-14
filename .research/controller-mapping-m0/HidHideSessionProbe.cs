// Bounded Windows M0 experiment for the HidHide 1.5.230 release ABI.
// Persistent driver settings require a durable journal and separate recovery process.
using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;
using System.Web.Script.Serialization;
using Microsoft.Win32.SafeHandles;

class HidHideSessionProbe {
    const int Limit = 65536;
    static JavaScriptSerializer Json = new JavaScriptSerializer { MaxJsonLength = 1024 * 1024 };
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    static extern SafeFileHandle CreateFile(string path, uint access, uint share, IntPtr security, uint disposition, uint flags, IntPtr template);
    [DllImport("kernel32.dll", SetLastError=true)]
    static extern bool DeviceIoControl(SafeFileHandle handle, uint code, byte[] input, uint inputLength, byte[] output, uint outputLength, out uint returned, IntPtr overlapped);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    static extern uint QueryDosDevice(string name, StringBuilder target, int length);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    static extern uint GetFinalPathNameByHandle(SafeFileHandle handle, StringBuilder path, uint length, uint flags);

    public class State {
        public bool active { get; set; }
        public bool inverse { get; set; }
        public string white { get; set; }
        public string black { get; set; }
    }
    public class Journal {
        public int schema { get; set; }
        public State before { get; set; }
        public State allowed { get; set; }
        public State hidden { get; set; }
    }
    static void Require(bool value, string message) { if (!value) throw new InvalidOperationException(message); }
    static SafeFileHandle Open() {
        var handle = CreateFile(@"\\.\HidHide", 0x80000000, 7, IntPtr.Zero, 3, 0x80, IntPtr.Zero);
        if (handle.IsInvalid) { int error=Marshal.GetLastWin32Error(); handle.Dispose(); throw new Win32Exception(error); }
        return handle; // The driver enforces single-client access across the whole transaction.
    }
    static uint Call(SafeFileHandle handle, uint function, byte[] input, byte[] output) {
        uint returned;
        uint code=(32769u << 16) | (1u << 14) | (function << 2);
        if (!DeviceIoControl(handle,code,input,(uint)(input == null ? 0 : input.Length),
                             output,(uint)(output == null ? 0 : output.Length),out returned,IntPtr.Zero))
            throw new Win32Exception(Marshal.GetLastWin32Error());
        return returned;
    }
    static string GetList(SafeFileHandle handle, uint function) {
        uint length=Call(handle,function,null,null);
        Require(length >= 4 && length <= Limit && length % 2 == 0, "Unexpected driver list size.");
        byte[] bytes=new byte[length];
        Require(Call(handle,function,null,bytes) == length, "Driver list size changed.");
        string list=Encoding.Unicode.GetString(bytes);
        Require(list.EndsWith("\0\0",StringComparison.Ordinal), "Invalid driver MULTI_SZ.");
        return Convert.ToBase64String(bytes);
    }
    static bool GetFlag(SafeFileHandle handle, uint function) {
        byte[] bytes=new byte[1];
        Require(Call(handle,function,null,bytes) == 1 && bytes[0] <= 1, "Invalid driver flag.");
        return bytes[0] == 1;
    }
    static State Read(SafeFileHandle handle) {
        return new State { white=GetList(handle,2048), black=GetList(handle,2050),
                           active=GetFlag(handle,2052), inverse=GetFlag(handle,2054) };
    }
    static string[] Entries(string encoded) {
        return Encoding.Unicode.GetString(Convert.FromBase64String(encoded)).Split(new char[]{'\0'},StringSplitOptions.RemoveEmptyEntries);
    }
    static string Add(string encoded, string entry) {
        Require(!Entries(encoded).Contains(entry,StringComparer.OrdinalIgnoreCase), "Selection already exists in policy.");
        string value=Encoding.Unicode.GetString(Convert.FromBase64String(encoded)).TrimEnd('\0');
        byte[] bytes=Encoding.Unicode.GetBytes((value.Length == 0 ? "" : value+"\0") + entry+"\0\0");
        Require(bytes.Length <= Limit, "Policy exceeds bounded size.");
        return Convert.ToBase64String(bytes);
    }
    static bool Same(State a, State b) {
        return a.active == b.active && a.inverse == b.inverse &&
            Entries(a.white).SequenceEqual(Entries(b.white),StringComparer.OrdinalIgnoreCase) &&
            Entries(a.black).SequenceEqual(Entries(b.black),StringComparer.OrdinalIgnoreCase);
    }
    static bool Known(State current, Journal journal) {
        return Same(current,journal.before) || Same(current,journal.allowed) || Same(current,journal.hidden);
    }
    static void SetList(SafeFileHandle handle, uint function, string encoded) {
        byte[] bytes=Convert.FromBase64String(encoded);
        Require(bytes.Length <= Limit, "Policy exceeds bounded size.");
        Call(handle,function,bytes,null);
    }
    static void Restore(SafeFileHandle handle, Journal journal) {
        State current=Read(handle);
        Require(Known(current,journal), "External policy change: retained journal; refusing to overwrite.");
        // Every recognized state differs only by this session's two additions.
        // Remove the selected-device addition before revoking the capture exception.
        if (!Same(current,journal.before)) {
            SetList(handle,2051,journal.before.black);
            SetList(handle,2049,journal.before.white);
        }
        Require(Same(Read(handle),journal.before), "Restoration readback mismatch.");
    }
    static void Save(string path, object value) {
        byte[] bytes=Encoding.UTF8.GetBytes(Json.Serialize(value));
        using(var file=new FileStream(path,FileMode.CreateNew,FileAccess.Write,FileShare.Read)) {
            file.Write(bytes,0,bytes.Length);
            file.Flush(true);
        }
    }
    static string ImagePath(string path) {
        path=Path.GetFullPath(path);
        Require(path.Length > 3 && path[1] == ':' && File.Exists(path), "Existing local executable required.");
        var final=new StringBuilder(4096);
        using(var handle=CreateFile(path,0,7,IntPtr.Zero,3,0x80,IntPtr.Zero)) {
            if (handle.IsInvalid) throw new Win32Exception(Marshal.GetLastWin32Error());
            uint length=GetFinalPathNameByHandle(handle,final,4096,0);
            Require(length > 0 && length < 4096, "Cannot resolve executable path.");
        }
        Require(final.ToString().StartsWith(@"\\?\",StringComparison.Ordinal), "Unsupported executable location.");
        string canonical=final.ToString().Substring(4);
        Require(String.Equals(path,canonical,StringComparison.OrdinalIgnoreCase), "Executable aliases are unsupported by this probe.");
        var device=new StringBuilder(4096);
        if (QueryDosDevice(canonical.Substring(0,2),device,4096) == 0) throw new Win32Exception(Marshal.GetLastWin32Error());
        return device.ToString()+canonical.Substring(2);
    }
    static string Quote(string argument) {
        Require(argument.IndexOf('"') < 0 && argument.IndexOf('\0') < 0 && !argument.EndsWith("\\"), "Unsupported argument.");
        return "\""+argument+"\"";
    }
    static object Probe(string executable) {
        var start=new ProcessStartInfo(executable) { UseShellExecute=false, CreateNoWindow=true, RedirectStandardOutput=true, RedirectStandardError=true };
        using(var child=Process.Start(start)) {
            var output=child.StandardOutput.ReadToEndAsync();
            var error=child.StandardError.ReadToEndAsync();
            if (!child.WaitForExit(10000)) { child.Kill(); child.WaitForExit(); throw new TimeoutException("Raw Input probe exceeded ten seconds."); }
            Require(output.Wait(1000) && error.Wait(1000), "Probe output did not complete.");
            Require(child.ExitCode == 0, "Raw Input probe failed: "+error.Result);
            Require(output.Result.Length < Limit, "Probe output exceeds bound.");
            return Json.DeserializeObject(output.Result);
        }
    }
    static void Watch(int parentId, long startTicks, string journalPath) {
        using(var parent=Process.GetProcessById(parentId)) {
            Require(parent.StartTime.ToUniversalTime().Ticks == startTicks, "Parent identity mismatch.");
            Save(journalPath+".ready",new { recoveryReady=true });
            if (!parent.WaitForExit(120000)) { parent.Kill(); parent.WaitForExit(5000); }
        }
        Journal journal=Json.Deserialize<Journal>(File.ReadAllText(journalPath));
        Require(journal.schema == 1, "Journal schema.");
        using(var handle=Open()) Restore(handle,journal);
        Save(journalPath+".recovered",new { restored=true });
    }
    static Dictionary<string,object> Object(object value) { return (Dictionary<string,object>)value; }
    static Dictionary<string,object>[] Devices(object sample, string field, int vendor, int product) {
        return ((object[])Object(sample)[field]).Select(Object)
            .Where(item => Convert.ToInt32(item["vendor"]) == vendor && Convert.ToInt32(item["product"]) == product).ToArray();
    }
    static void ValidateSample(object sample, int vendor, int product, bool accessible) {
        var raw=Devices(sample,"devices",vendor,product);
        Require(raw.Length == 1, "Selected vendor/product must identify one collection in this probe.");
        Require(Convert.ToBoolean(raw[0]["readHandleOpened"]) == accessible, "Unexpected HID handle access.");
        Require(Convert.ToInt32(raw[0]["readHandleError"]) == (accessible ? 0 : 5), "Unexpected HID open error.");
        Require(Devices(sample,"directInputDevices",vendor,product).Length == (accessible ? 1 : 0),
                "Unexpected DirectInput enumeration visibility.");
        if (accessible) {
            var reports=Object(Object(sample)["reports"]);
            Require(Convert.ToInt32(reports[(string)raw[0]["id"]]) > 0, "No selected-source reports received.");
        }
    }
    static void SelfTest() {
        string empty=Convert.ToBase64String(Encoding.Unicode.GetBytes("\0\0"));
        var before=new State { active=true, inverse=false, white=Add(empty,@"\Device\Volume\existing.exe"), black=Add(empty,@"HID\EXISTING") };
        var allowed=new State { active=true, inverse=false, white=Add(before.white,@"\Device\Volume\capture.exe"), black=before.black };
        var hidden=new State { active=true, inverse=false, white=allowed.white, black=Add(before.black,@"HID\TEST") };
        var journal=new Journal { schema=1, before=before, allowed=allowed, hidden=hidden };
        Require(Known(before,journal) && Known(allowed,journal) && Known(hidden,journal), "Expected recovery stages.");
        var conflict=new State { active=true, inverse=false, white=hidden.white, black=Add(hidden.black,@"HID\EXTERNAL") };
        Require(!Known(conflict,journal), "External device addition must conflict.");
        conflict=new State { active=true, inverse=true, white=hidden.white, black=hidden.black };
        Require(!Known(conflict,journal), "External mode edit must conflict.");
        conflict=new State { active=true, inverse=false, white=before.white, black=hidden.black };
        Require(!Known(conflict,journal), "Unknown partial state must conflict.");
        bool rejected=false;
        try { Add(before.black,@"hid\existing"); } catch(InvalidOperationException) { rejected=true; }
        Require(rejected, "Case-insensitive duplicate must be rejected.");
        Require(Entries(Add(empty,@"HID\FIRST")).Length == 1, "Empty MULTI_SZ insertion.");
        Console.WriteLine("HidHide journal self-test passed (8 assertions).");
    }


    static int Session(string instance, string capture, string observer, string journalPath, string mode) {
        Require(mode == "normal" || mode == "fail" || mode == "crash", "Unknown test mode.");
        Require(instance.StartsWith(@"HID\VID_231D&PID_",StringComparison.OrdinalIgnoreCase) &&
                instance.Length <= 512 && instance.IndexOf('\0') < 0, "Explicit MOZA HID instance required.");
        string captureImage=ImagePath(capture), observerImage=ImagePath(observer);
        Require(!String.Equals(captureImage,observerImage,StringComparison.OrdinalIgnoreCase), "Different executable paths required.");
        Require(!File.Exists(journalPath), "Refusing to reuse a journal.");
        int vendor=Convert.ToInt32(instance.Substring(8,4),16), product=Convert.ToInt32(instance.Substring(17,4),16);
        object baseline=Probe(observer), allowed=null, blocked=null, after=null;
        ValidateSample(baseline,vendor,product,true);
        using(var handle=Open()) {
            State before=Read(handle);
            Require(before.active && !before.inverse, "Existing active normal mode required; global mode is never changed.");
            Require(!Entries(before.white).Contains(observerImage,StringComparer.OrdinalIgnoreCase), "Observer is allowlisted.");
            var journal=new Journal { schema=1, before=before,
                allowed=new State { active=before.active, inverse=before.inverse, black=before.black, white=Add(before.white,captureImage) } };
            journal.hidden=new State { active=before.active, inverse=before.inverse, white=journal.allowed.white, black=Add(before.black,instance) };
            Save(journalPath,journal); // Persist all possible states before the first driver write.
            var own=Process.GetCurrentProcess();
            var guardStart=new ProcessStartInfo(own.MainModule.FileName,
                "watch "+own.Id+" "+own.StartTime.ToUniversalTime().Ticks+" "+Quote(journalPath)) {
                UseShellExecute=false, CreateNoWindow=true
            };
            using(var guard=Process.Start(guardStart)) {
                var wait=Stopwatch.StartNew();
                while (!File.Exists(journalPath+".ready") && !guard.HasExited && wait.ElapsedMilliseconds < 5000) Thread.Sleep(20);
                Require(File.Exists(journalPath+".ready") && !guard.HasExited, "Recovery process did not start; no policy changed.");
            }
            try {
                SetList(handle,2049,journal.allowed.white);
                Require(Same(Read(handle),journal.allowed), "Capture exception readback mismatch.");
                SetList(handle,2051,journal.hidden.black);
                Require(Same(Read(handle),journal.hidden), "Hidden device readback mismatch.");
                if (mode == "crash") Environment.Exit(86); // Deliberately bypass finally; guardian must restore.
                if (mode == "fail") throw new InvalidOperationException("Injected session failure.");
                allowed=Probe(capture);
                blocked=Probe(observer);
            } finally { Restore(handle,journal); }
        }
        after=Probe(observer);
        ValidateSample(allowed,vendor,product,true);
        ValidateSample(blocked,vendor,product,false);
        ValidateSample(after,vendor,product,true);
        Console.WriteLine(Json.Serialize(new { mode=mode, baseline=baseline, captureWhileHidden=allowed,
            observerWhileHidden=blocked, observerAfterRestore=after, restored=true }));
        return 0;
    }
    static int Main(string[] args) {
        try {
            if (args.Length == 1 && args[0] == "selftest") { SelfTest(); return 0; }
            if (args.Length == 1 && args[0] == "snapshot") {
                using(var handle=Open()) {
                    State state=Read(handle);
                    Console.WriteLine(Json.Serialize(new { active=state.active, inverse=state.inverse,
                        applications=Entries(state.white).Length, devices=Entries(state.black).Length }));
                }
                return 0;
            }
            if (args.Length == 4 && args[0] == "watch") {
                try { Watch(int.Parse(args[1]),long.Parse(args[2]),args[3]); }
                catch(Exception error) { Save(args[3]+".recovery-error",new { error=error.Message }); return 1; }
                return 0;
            }
            if (args.Length == 6 && args[0] == "session") return Session(args[1],args[2],args[3],args[4],args[5]);
            throw new ArgumentException("Use snapshot or session <MOZA instance> <capture.exe> <observer.exe> <new journal.json> <normal|fail|crash>.");
        } catch(Exception error) { Console.Error.WriteLine(error.Message); return 1; }
    }
}
