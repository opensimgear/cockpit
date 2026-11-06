// M0 bounded Raw Input experiment. Registers controller usages only, never keyboard/mouse.
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using System.Text;
using System.Threading;
using System.Web.Script.Serialization;
using Microsoft.Win32.SafeHandles;

class RawInputProbe {
    const uint RID_INPUT = 0x10000003, RIDI_PREPARSEDDATA = 0x20000005,
        RIDI_DEVICENAME = 0x20000007, RIDI_DEVICEINFO = 0x2000000b;
    [StructLayout(LayoutKind.Sequential)] struct Device { public IntPtr handle; public uint type; }
    [StructLayout(LayoutKind.Sequential)] struct Registration { public ushort page, usage; public uint flags; public IntPtr target; }
    [StructLayout(LayoutKind.Sequential)] struct Message {
        public IntPtr hwnd; public uint message; public UIntPtr wParam; public IntPtr lParam;
        public uint time; public int x, y; public uint privateValue;
    }
    [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)] struct WindowClass {
        public uint size, style; public WndProc proc; public int clsExtra, wndExtra;
        public IntPtr instance, icon, cursor, background;
        public string menu, name; public IntPtr smallIcon;
    }
    delegate IntPtr WndProc(IntPtr hwnd, uint message, UIntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll")] static extern uint GetRawInputDeviceList([In,Out] Device[] list, ref uint count, uint size);
    [DllImport("user32.dll", CharSet=CharSet.Unicode, EntryPoint="GetRawInputDeviceInfoW")] static extern uint DeviceName(IntPtr device, uint command, StringBuilder name, ref uint size);
    [DllImport("user32.dll", EntryPoint="GetRawInputDeviceInfoW")] static extern uint DeviceInfo(IntPtr device, uint command, IntPtr info, ref uint size);
    [DllImport("hid.dll")] static extern int HidP_GetUsages(int type, ushort page, ushort collection,
        [Out] ushort[] usages, ref uint count, IntPtr data, byte[] report, uint size);
    [DllImport("user32.dll")] static extern uint GetRawInputData(IntPtr input, uint command, IntPtr data, ref uint size, uint headerSize);
    [DllImport("user32.dll", SetLastError=true)] static extern bool RegisterRawInputDevices(Registration[] devices, uint count, uint size);
    [DllImport("user32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern ushort RegisterClassEx(ref WindowClass cls);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern IntPtr CreateWindowEx(uint ex, string cls, string title, uint style, int x, int y, int w, int h, IntPtr parent, IntPtr menu, IntPtr instance, IntPtr param);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern IntPtr DefWindowProc(IntPtr hwnd, uint message, UIntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll")] static extern bool PeekMessage(out Message message, IntPtr hwnd, uint min, uint max, uint remove);
    [DllImport("user32.dll")] static extern IntPtr DispatchMessage(ref Message message);
    [DllImport("user32.dll", SetLastError=true)]
    static extern uint MsgWaitForMultipleObjectsEx(uint count, IntPtr handles, uint timeout, uint wakeMask, uint flags);
    [DllImport("user32.dll")] static extern bool DestroyWindow(IntPtr window);
    [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr window, out uint pid);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    static extern SafeFileHandle CreateFile(string path, uint access, uint share, IntPtr security, uint disposition, uint flags, IntPtr template);
    static Dictionary<IntPtr,string> names = new Dictionary<IntPtr,string>();
    static Dictionary<string,int> reports = new Dictionary<string,int>();
    static HashSet<string> changes = new HashSet<string>();
    static HashSet<IntPtr> vjoyDevices = new HashSet<IntPtr>();
    static List<object> vjoyButtons = new List<object>();
    static Dictionary<IntPtr,bool> lastButton = new Dictionary<IntPtr,bool>();
    static WndProc callback = Handle;
    static long vjoyCaptureStarted;
    static uint HeaderSize { get { return (uint)(8 + IntPtr.Size * 2); } }
    static string Hash(byte[] bytes) { using(var hash=SHA256.Create()) return BitConverter.ToString(hash.ComputeHash(bytes)).Replace("-","").Substring(0,16); }

    static void ObserveVjoyButton(IntPtr device, string name, IntPtr bytes, uint item, uint count, long receivedTicks) {
        uint size = 0;
        if (DeviceInfo(device, RIDI_PREPARSEDDATA, IntPtr.Zero, ref size) == uint.MaxValue
            || size == 0 || size > 65536) return;
        var descriptor = Marshal.AllocHGlobal((int)size);
        try {
            if (DeviceInfo(device, RIDI_PREPARSEDDATA, descriptor, ref size) == uint.MaxValue) return;
            var usages = new ushort[128];
            var report = new byte[item];
            for (uint index = 0; index < count && vjoyButtons.Count < 256; index++) {
                Marshal.Copy(IntPtr.Add(bytes, (int)(index * item)), report, 0, report.Length);
                uint length = (uint)usages.Length;
                int status = HidP_GetUsages(0, 9, 0, usages, ref length, descriptor, report, item);
                bool pressed = status == 0x00110000 && Array.IndexOf(usages, (ushort)1, 0, (int)length) >= 0;
                bool previous;
                if (status != 0x00110000 || !lastButton.TryGetValue(device, out previous) || previous != pressed) {
                    vjoyButtons.Add(new { device=name, report=report[0], pressed=pressed, status=status, receivedTicks=receivedTicks });
                    lastButton[device] = pressed;
                }
            }
        } finally { Marshal.FreeHGlobal(descriptor); }
    }

    static IntPtr Handle(IntPtr hwnd, uint message, UIntPtr wParam, IntPtr lParam) {
        if (message == 0xff) {
            long receivedTicks = Stopwatch.GetTimestamp() - vjoyCaptureStarted;
            uint size = 0;
            GetRawInputData(lParam, RID_INPUT, IntPtr.Zero, ref size, HeaderSize);
            if (size >= HeaderSize + 8 && size <= 65536) {
                IntPtr buffer = Marshal.AllocHGlobal((int)size);
                try {
                    if (GetRawInputData(lParam, RID_INPUT, buffer, ref size, HeaderSize) == size
                        && Marshal.ReadInt32(buffer) == 2) {
                        IntPtr device = Marshal.ReadIntPtr(buffer, 8);
                        string name;
                        if (names.TryGetValue(device, out name)) {
                            uint item = (uint)Marshal.ReadInt32(buffer, (int)HeaderSize);
                            uint count = (uint)Marshal.ReadInt32(buffer, (int)HeaderSize + 4);
                            if (item > 0 && (ulong)item * count <= size - HeaderSize - 8) {
                                reports[name] += (int)count;
                                if (vjoyButtons.Count < 256 && vjoyDevices.Contains(device)) {
                                    ObserveVjoyButton(device, name, IntPtr.Add(buffer, (int)HeaderSize + 8), item, count, receivedTicks);
                                }
                                if (changes.Count < 256) {
                                    byte[] bytes = new byte[item * count];
                                    Marshal.Copy(IntPtr.Add(buffer, (int)HeaderSize + 8), bytes, 0, bytes.Length);
                                    changes.Add(name + ":" + Hash(bytes));
                                }
                            }
                        }
                    }
                } finally { Marshal.FreeHGlobal(buffer); }
            }
        }
        return DefWindowProc(hwnd, message, wParam, lParam);
    }

    [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)]
    struct DirectInputInstance {
        public uint size;
        public Guid instance, product;
        public uint deviceType;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst=260)] public string instanceName;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst=260)] public string productName;
        public Guid forceFeedback;
        public ushort page, usage;
    }
    [DllImport("dinput8.dll", CallingConvention=CallingConvention.StdCall)]
    static extern int DirectInput8Create(IntPtr instance, uint version, ref Guid id, out IntPtr api, IntPtr outer);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode)]
    static extern IntPtr GetModuleHandle(string name);
    [UnmanagedFunctionPointer(CallingConvention.StdCall)]
    delegate int DeviceCallback(IntPtr device, IntPtr context);
    [UnmanagedFunctionPointer(CallingConvention.StdCall)]
    delegate int EnumDirectDevices(IntPtr api, uint kind, DeviceCallback callback, IntPtr context, uint flags);
    static List<object> DirectInputDevices() {
        Guid id=new Guid("BF798031-483A-4DA2-AA99-5D64ED369700");
        IntPtr api;
        int result=DirectInput8Create(GetModuleHandle(null),0x800,ref id,out api,IntPtr.Zero);
        if(result < 0) Marshal.ThrowExceptionForHR(result);
        try {
            var devices=new List<object>();
            var enumerate=(EnumDirectDevices)Marshal.GetDelegateForFunctionPointer(
                Marshal.ReadIntPtr(Marshal.ReadIntPtr(api),4*IntPtr.Size),typeof(EnumDirectDevices));
            DeviceCallback callback=delegate(IntPtr data, IntPtr context) {
                if (devices.Count >= 256) return 0;
                var device=(DirectInputInstance)Marshal.PtrToStructure(data,typeof(DirectInputInstance));
                byte[] product=device.product.ToByteArray();
                devices.Add(new { id=Hash(device.instance.ToByteArray()), vendor=BitConverter.ToUInt16(product,0),
                    product=BitConverter.ToUInt16(product,2), usage=device.usage });
                return 1;
            };
            result=enumerate(api,4,callback,IntPtr.Zero,1); // Game controllers; attached only.
            GC.KeepAlive(callback);
            if(result < 0) Marshal.ThrowExceptionForHR(result);
            return devices;
        } finally { Marshal.Release(api); }
    }


    static int Main(string[] args) {
        bool observeVjoy = args.Length == 1 && args[0] == "--vjoy-button";
        if (args.Length > 0 && !observeVjoy) throw new ArgumentException("Expected --vjoy-button or no arguments.");
        var devices = new List<object>();
        uint count = 0;
        if (GetRawInputDeviceList(null, ref count, (uint)Marshal.SizeOf(typeof(Device))) == uint.MaxValue || count > 256)
            throw new Exception("Raw Input enumeration failed or exceeds bounded device count.");
        Device[] list = new Device[count];
        uint read = GetRawInputDeviceList(list, ref count, (uint)Marshal.SizeOf(typeof(Device)));
        if (read == uint.MaxValue || read > list.Length) throw new Exception("Device list changed; retry enumeration.");
        for (int i = 0; i < read; ++i) {
            if (list[i].type != 2) continue;
            IntPtr info = Marshal.AllocHGlobal(32);
            try {
                uint size = 32;
                Marshal.WriteInt32(info, 32);
                if (DeviceInfo(list[i].handle, RIDI_DEVICEINFO, info, ref size) == uint.MaxValue) continue;
                ushort page = (ushort)Marshal.ReadInt16(info, 20), usage = (ushort)Marshal.ReadInt16(info, 22);
                if (page != 1 || (usage != 4 && usage != 5 && usage != 8)) continue;
                uint length = 0;
                DeviceName(list[i].handle, RIDI_DEVICENAME, null, ref length);
                if (length == 0 || length > 4096) continue;
                var path = new StringBuilder((int)length + 1);
                if (DeviceName(list[i].handle, RIDI_DEVICENAME, path, ref length) == uint.MaxValue) continue;
                string id = Hash(Encoding.UTF8.GetBytes(path.ToString()));
                uint vendor=(uint)Marshal.ReadInt32(info,8), product=(uint)Marshal.ReadInt32(info,12);
                names[list[i].handle] = id;
                reports[id] = 0;
                if (observeVjoy && vendor == 0x1234 && product == 0xbead) vjoyDevices.Add(list[i].handle);
                bool readable;
                int openError;
                using(var file=CreateFile(path.ToString(),0x80000000,3,IntPtr.Zero,3,0x40000000,IntPtr.Zero)) {
                    readable=!file.IsInvalid;
                    openError=readable ? 0 : Marshal.GetLastWin32Error();
                }
                devices.Add(new { id=id, vendor=vendor, product=product, usage=usage,
                    readHandleOpened=readable, readHandleError=openError,
                    virtualVjoy=(vendor == 0x1234 && product == 0xbead) });
            } finally { Marshal.FreeHGlobal(info); }
        }
        var cls = new WindowClass { size=(uint)Marshal.SizeOf(typeof(WindowClass)), proc=callback, name="OSGMappingM0RawInput" };
        if (RegisterClassEx(ref cls) == 0) throw new Exception("Could not register message window.");
        IntPtr hwnd=CreateWindowEx(0,cls.name,"",0,0,0,0,0,new IntPtr(-3),IntPtr.Zero,IntPtr.Zero,IntPtr.Zero);
        if (hwnd == IntPtr.Zero) throw new Exception("Could not create message-only window.");
        var registrations = new Registration[3];
        ushort[] usages = {4,5,8};
        for(int i=0;i<3;++i) registrations[i]=new Registration {page=1,usage=usages[i],flags=0x100,target=hwnd};
        try {
            if(!RegisterRawInputDevices(registrations,3,(uint)Marshal.SizeOf(typeof(Registration))))
                throw new Exception("Background controller registration failed: "+Marshal.GetLastWin32Error());
            if (observeVjoy) {
                vjoyCaptureStarted = Stopwatch.GetTimestamp();
                Console.WriteLine("ready");
                Console.Out.Flush();
            }
            uint foregroundPid;
            GetWindowThreadProcessId(GetForegroundWindow(),out foregroundPid);
            string foreground = "unavailable";
            try { foreground=Process.GetProcessById((int)foregroundPid).ProcessName; } catch {}
            var clock=Stopwatch.StartNew();
            while(clock.ElapsedMilliseconds < 5000) {
                Message message;
                // Bound one drain to ensure a flood cannot keep the probe alive forever.
                for(int i=0;i<256 && PeekMessage(out message,IntPtr.Zero,0,0,1);++i) DispatchMessage(ref message);
                if (observeVjoy) {
                    uint remaining = (uint)Math.Max(0, 5000 - clock.ElapsedMilliseconds);
                    const uint allInput = 0x04ff, inputAvailable = 4;
                    if (MsgWaitForMultipleObjectsEx(0, IntPtr.Zero, remaining, allInput, inputAvailable) == uint.MaxValue)
                        throw new Exception("Raw Input message wait failed: " + Marshal.GetLastWin32Error());
                } else {
                    Thread.Sleep(1);
                }
            }
            var serializer = new JavaScriptSerializer();
            if (observeVjoy) {
                Console.WriteLine(serializer.Serialize(new {devices=devices, reports=reports, buttonEvents=vjoyButtons, receiverTickHz=Stopwatch.Frequency}));
            } else {
                Console.WriteLine(serializer.Serialize(new {
                    devices=devices, directInputDevices=DirectInputDevices(), reports=reports, distinctReportHashes=changes.Count,
                    backgroundRegistration=true, foregroundProcess=foreground, durationMs=clock.ElapsedMilliseconds
                }));
            }
        } finally {
            for(int i=0;i<3;++i) { registrations[i].flags=1; registrations[i].target=IntPtr.Zero; }
            RegisterRawInputDevices(registrations,3,(uint)Marshal.SizeOf(typeof(Registration)));
            DestroyWindow(hwnd);
        }
        return 0;
    }
}
