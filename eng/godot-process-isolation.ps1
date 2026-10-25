# Process creation for bounded Godot validation only. No persistent policy changes.
if (!('RsfSyncIsolationProcess' -as [type])) {
Add-Type -TypeDefinition @'
using System;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text;
public static class RsfSyncIsolationProcess {
    [StructLayout(LayoutKind.Sequential)] struct Security { public int length; public IntPtr descriptor; public int inherit; }
    [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)] struct Startup {
        public int size; public string reserved, desktop, title;
        public int x, y, width, height, charsX, charsY, fill, flags;
        public short show, reservedSize; public IntPtr reservedData, input, output, error;
    }
    [StructLayout(LayoutKind.Sequential)] struct StartupEx { public Startup startup; public IntPtr attributes; }
    [StructLayout(LayoutKind.Sequential)] struct Information { public IntPtr process, thread; public int pid, tid; }
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool InitializeProcThreadAttributeList(IntPtr list, int count, int flags, ref IntPtr size);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool UpdateProcThreadAttribute(IntPtr list, uint flags, IntPtr attribute, IntPtr value, IntPtr size, IntPtr previous, IntPtr returned);
    [DllImport("kernel32.dll")] static extern void DeleteProcThreadAttributeList(IntPtr list);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern IntPtr CreateFileW(string path, uint access, uint share, ref Security security, uint creation, uint flags, IntPtr template);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern bool CreateProcessW(string application, StringBuilder command, IntPtr processAttributes, IntPtr threadAttributes, bool inherit, uint flags, IntPtr environment, string directory, ref StartupEx startup, out Information info);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool GetProcessMitigationPolicy(IntPtr process, int policy, out uint flags, IntPtr size);
    [DllImport("kernel32.dll", SetLastError=true)] static extern uint ResumeThread(IntPtr thread);
    [DllImport("kernel32.dll")] static extern bool TerminateProcess(IntPtr process, uint code);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
    static Exception Error(string name) { return new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error(), name); }
    public static Process Start(string exe, string arguments, string directory, string stdout, string stderr, bool disableExtensions, out uint actualPolicy) {
        IntPtr size=IntPtr.Zero, list=IntPtr.Zero, policy=IntPtr.Zero, handles=IntPtr.Zero;
        IntPtr input=IntPtr.Zero, output=IntPtr.Zero, error=IntPtr.Zero;
        Information info=new Information(); bool initialized=false;
        try {
            Security security=new Security(); security.length=Marshal.SizeOf<Security>(); security.inherit=1;
            input=CreateFileW("NUL",0x80000000,3,ref security,3,0,IntPtr.Zero);
            output=CreateFileW(stdout,0x40000000,1,ref security,2,0,IntPtr.Zero);
            error=CreateFileW(stderr,0x40000000,1,ref security,2,0,IntPtr.Zero);
            if (input==new IntPtr(-1)||output==new IntPtr(-1)||error==new IntPtr(-1)) throw Error("Create diagnostic streams");
            InitializeProcThreadAttributeList(IntPtr.Zero,2,0,ref size);
            list=Marshal.AllocHGlobal(size);
            if (!InitializeProcThreadAttributeList(list,2,0,ref size)) throw Error("Initialize process attributes");
            initialized=true;
            policy=Marshal.AllocHGlobal(8); Marshal.WriteInt64(policy,disableExtensions ? 1L<<32 : 0L);
            if (!UpdateProcThreadAttribute(list,0,new IntPtr(0x20007),policy,new IntPtr(8),IntPtr.Zero,IntPtr.Zero)) throw Error("Set extension-point mitigation");
            handles=Marshal.AllocHGlobal(IntPtr.Size*3);
            Marshal.WriteIntPtr(handles,0,input); Marshal.WriteIntPtr(handles,IntPtr.Size,output); Marshal.WriteIntPtr(handles,IntPtr.Size*2,error);
            if (!UpdateProcThreadAttribute(list,0,new IntPtr(0x20002),handles,new IntPtr(IntPtr.Size*3),IntPtr.Zero,IntPtr.Zero)) throw Error("Set inherited diagnostic handles");
            StartupEx startup=new StartupEx(); startup.startup.size=Marshal.SizeOf<StartupEx>(); startup.attributes=list;
            startup.startup.flags=0x101; startup.startup.show=1;
            startup.startup.input=input; startup.startup.output=output; startup.startup.error=error;
            if (!CreateProcessW(exe,new StringBuilder("\""+exe+"\" "+arguments),IntPtr.Zero,IntPtr.Zero,true,0x80004,IntPtr.Zero,directory,ref startup,out info)) throw Error("Create isolated engine process");
            if (!GetProcessMitigationPolicy(info.process,6,out actualPolicy,new IntPtr(4))) throw Error("Query effective mitigation");
            if (disableExtensions && (actualPolicy&1)==0) throw new InvalidOperationException("Requested mitigation is not active");
            Process process=Process.GetProcessById(info.pid); IntPtr ownedHandle=process.Handle;
            if (ResumeThread(info.thread)==uint.MaxValue) throw Error("Resume engine process");
            CloseHandle(info.thread); info.thread=IntPtr.Zero; CloseHandle(info.process); info.process=IntPtr.Zero;
            return process;
        } catch {
            if (info.process!=IntPtr.Zero) TerminateProcess(info.process,9);
            throw;
        } finally {
            if (info.thread!=IntPtr.Zero) CloseHandle(info.thread);
            if (info.process!=IntPtr.Zero) CloseHandle(info.process);
            if (initialized) DeleteProcThreadAttributeList(list);
            foreach (IntPtr p in new[]{list,policy,handles}) if(p!=IntPtr.Zero) Marshal.FreeHGlobal(p);
            foreach (IntPtr h in new[]{input,output,error}) if(h!=IntPtr.Zero && h!=new IntPtr(-1)) CloseHandle(h);
        }
    }
}
'@
}
