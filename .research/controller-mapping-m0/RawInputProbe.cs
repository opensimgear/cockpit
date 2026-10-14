// M0 bounded Raw Input experiment. Registers controller usages only, never keyboard/mouse.
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using System.Text;
using System.Threading;
using System.Web.Script.Serialization;

class RawInputProbe {
    const uint RID_INPUT = 0x10000003, RIDI_DEVICENAME = 0x20000007, RIDI_DEVICEINFO = 0x2000000b;
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
    [DllImport("user32.dll")] static extern uint GetRawInputData(IntPtr input, uint command, IntPtr data, ref uint size, uint headerSize);
    [DllImport("user32.dll", SetLastError=true)] static extern bool RegisterRawInputDevices(Registration[] devices, uint count, uint size);
    [DllImport("user32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern ushort RegisterClassEx(ref WindowClass cls);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern IntPtr CreateWindowEx(uint ex, string cls, string title, uint style, int x, int y, int w, int h, IntPtr parent, IntPtr menu, IntPtr instance, IntPtr param);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern IntPtr DefWindowProc(IntPtr hwnd, uint message, UIntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll")] static extern bool PeekMessage(out Message message, IntPtr hwnd, uint min, uint max, uint remove);
    [DllImport("user32.dll")] static extern IntPtr DispatchMessage(ref Message message);
    [DllImport("user32.dll")] static extern bool DestroyWindow(IntPtr window);
    [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr window, out uint pid);
    static Dictionary<IntPtr,string> names = new Dictionary<IntPtr,string>();
    static Dictionary<string,int> reports = new Dictionary<string,int>();
    static HashSet<string> changes = new HashSet<string>();
    static WndProc callback = Handle;
    static uint HeaderSize { get { return (uint)(8 + IntPtr.Size * 2); } }
    static string Hash(byte[] bytes) { using(var hash=SHA256.Create()) return BitConverter.ToString(hash.ComputeHash(bytes)).Replace("-","").Substring(0,16); }

    static IntPtr Handle(IntPtr hwnd, uint message, UIntPtr wParam, IntPtr lParam) {
        if (message == 0xff) {
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

    static int Main() {
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
                devices.Add(new { id=id, vendor=vendor, product=product, usage=usage,
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
            uint foregroundPid;
            GetWindowThreadProcessId(GetForegroundWindow(),out foregroundPid);
            string foreground = "unavailable";
            try { foreground=Process.GetProcessById((int)foregroundPid).ProcessName; } catch {}
            var clock=Stopwatch.StartNew();
            while(clock.ElapsedMilliseconds < 5000) {
                Message message;
                // Bound one drain to ensure a flood cannot keep the probe alive forever.
                for(int i=0;i<256 && PeekMessage(out message,IntPtr.Zero,0,0,1);++i) DispatchMessage(ref message);
                Thread.Sleep(1);
            }
            Console.WriteLine(new JavaScriptSerializer().Serialize(new {
                devices=devices, reports=reports, distinctReportHashes=changes.Count,
                backgroundRegistration=true, foregroundProcess=foreground, durationMs=clock.ElapsedMilliseconds
            }));
        } finally {
            for(int i=0;i<3;++i) { registrations[i].flags=1; registrations[i].target=IntPtr.Zero; }
            RegisterRawInputDevices(registrations,3,(uint)Marshal.SizeOf(typeof(Registration)));
            DestroyWindow(hwnd);
        }
        return 0;
    }
}
