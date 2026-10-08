// Owned test launcher for Bun.spawn([exe, "app-server", ...]); no service/install actions.
using System;
using System.Runtime.InteropServices;
using System.Text;
using System.IO;
using System.Collections.Generic;
using System.Text.RegularExpressions;
using System.Web.Script.Serialization;

internal static class ObserverProxyLauncher
{
    [StructLayout(LayoutKind.Sequential)] private struct StartupInfo
    {
        public uint size; public IntPtr reserved, desktop, title;
        public uint x, y, xSize, ySize, xCount, yCount, fill, flags;
        public ushort show, reservedSize; public IntPtr reservedBytes, input, output, error;
    }
    [StructLayout(LayoutKind.Sequential)] private struct ProcessInfo
    { public IntPtr process, thread; public uint processId, threadId; }
    [StructLayout(LayoutKind.Sequential)] private struct BasicLimits
    {
        public long processTime, jobTime; public uint flags;
        public UIntPtr minWorkingSet, maxWorkingSet; public uint activeProcesses;
        public UIntPtr affinity; public uint priority, scheduling;
    }
    [StructLayout(LayoutKind.Sequential)] private struct IoCounters
    { public ulong readOps, writeOps, otherOps, readBytes, writeBytes, otherBytes; }
    [StructLayout(LayoutKind.Sequential)] private struct ExtendedLimits
    {
        public BasicLimits basic; public IoCounters io;
        public UIntPtr processMemory, jobMemory, peakProcessMemory, peakJobMemory;
    }
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern IntPtr CreateJobObject(IntPtr attributes, string name);
    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool SetInformationJobObject(IntPtr job, int kind, ref ExtendedLimits limits, uint length);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern bool CreateProcess(string app, StringBuilder command, IntPtr processAttributes,
        IntPtr threadAttributes, bool inheritHandles, uint flags, IntPtr environment, string cwd,
        ref StartupInfo startup, out ProcessInfo process);
    [DllImport("kernel32.dll", SetLastError = true)] private static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);
    [DllImport("kernel32.dll", SetLastError = true)] private static extern uint ResumeThread(IntPtr thread);
    [DllImport("kernel32.dll", SetLastError = true)] private static extern uint WaitForSingleObject(IntPtr handle, uint milliseconds);
    [DllImport("kernel32.dll", SetLastError = true)] private static extern bool GetExitCodeProcess(IntPtr process, out uint code);
    [DllImport("kernel32.dll")] private static extern bool TerminateProcess(IntPtr process, uint code);
    [DllImport("kernel32.dll")] private static extern bool CloseHandle(IntPtr handle);
    [DllImport("kernel32.dll")] private static extern IntPtr GetStdHandle(int kind);

    // CommandLineToArgvW quoting: preserve empty values, quotes and trailing backslashes.
    private static string Quote(string value)
    {
        var text = new StringBuilder("\""); int slashes = 0;
        foreach (char character in value)
        {
            if (character == '\\') { slashes++; continue; }
            text.Append('\\', character == '"' ? slashes * 2 + 1 : slashes);
            text.Append(character); slashes = 0;
        }
        text.Append('\\', slashes * 2); return text.Append('"').ToString();
    }
    private static string CheckedPath(string value, bool directory)
    {
        if (String.IsNullOrEmpty(value) || value.Length > 2048 || !Path.IsPathRooted(value)
            || Path.GetFullPath(value) != value) throw new InvalidOperationException();
        foreach (char c in value) if (c < 32) throw new InvalidOperationException();
        string current = value;
        while (!String.IsNullOrEmpty(current))
        {
            if ((File.GetAttributes(current) & FileAttributes.ReparsePoint) != 0) throw new InvalidOperationException();
            current = Path.GetDirectoryName(current);
        }
        if (directory ? !Directory.Exists(value) : !File.Exists(value)) throw new InvalidOperationException();
        return value;
    }
    private static Dictionary<string, object> ReadSidecar(string file)
    {
        CheckedPath(file, false); string text;
        using (var stream = new FileStream(file, FileMode.Open, FileAccess.Read, FileShare.Read))
        {
            if (stream.Length < 1 || stream.Length > 8192) throw new InvalidOperationException();
            var bytes = new byte[(int)stream.Length]; int offset = 0;
            while (offset < bytes.Length)
            { int count = stream.Read(bytes, offset, bytes.Length - offset); if (count == 0) throw new InvalidOperationException(); offset += count; }
            if (stream.ReadByte() != -1) throw new InvalidOperationException();
            text = new UTF8Encoding(false, true).GetString(bytes);
        }
        const string token = @"""(?:[^""\\\x00-\x1f]|\\(?:[""\\/bfnrt]|u[0-9a-fA-F]{4}))*""";
        string pattern = @"\A\s*\{\s*""version""\s*:\s*1\s*,\s*""node""\s*:\s*" + token
            + @"\s*,\s*""script""\s*:\s*" + token + @"\s*,\s*""binary""\s*:\s*" + token
            + @"\s*,\s*""auditDirectory""\s*:\s*" + token + @"\s*\}\s*\z";
        if (!Regex.IsMatch(text, pattern, RegexOptions.None, TimeSpan.FromSeconds(1))) throw new InvalidOperationException();
        var config = new JavaScriptSerializer { MaxJsonLength = 8192, RecursionLimit = 2 }
            .Deserialize<Dictionary<string, object>>(text);
        foreach (string name in new[] { "node", "script", "binary", "auditDirectory" })
            CheckedPath((string)config[name], name == "auditDirectory");
        if (Path.GetFileName((string)config["script"]) != "claude-memory-observer-proxy.mjs"
            || !String.Equals(Path.GetExtension((string)config["node"]), ".exe", StringComparison.OrdinalIgnoreCase)
            || !String.Equals(Path.GetExtension((string)config["binary"]), ".exe", StringComparison.OrdinalIgnoreCase))
            throw new InvalidOperationException();
        return config;
    }
    private static int Main(string[] args)
    {
        IntPtr job = IntPtr.Zero; var child = new ProcessInfo();
        try
        {
            string sidecar = System.Reflection.Assembly.GetExecutingAssembly().Location + ".json";
            var config = ReadSidecar(sidecar);
            string node = (string)config["node"], script = (string)config["script"];
            var command = new StringBuilder(Quote(node)).Append(' ').Append(Quote(script))
                .Append(" --observer-sidecar ").Append(Quote(sidecar)).Append(" --");
            foreach (string argument in args) command.Append(' ').Append(Quote(argument));
            job = CreateJobObject(IntPtr.Zero, null);
            if (job == IntPtr.Zero) throw new InvalidOperationException();
            var limits = new ExtendedLimits(); limits.basic.flags = 0x2000; // KILL_ON_JOB_CLOSE; no breakaway.
            if (!SetInformationJobObject(job, 9, ref limits, (uint)Marshal.SizeOf(typeof(ExtendedLimits)))) throw new InvalidOperationException();
            var startup = new StartupInfo { size = (uint)Marshal.SizeOf(typeof(StartupInfo)), flags = 0x100,
                input = GetStdHandle(-10), output = GetStdHandle(-11), error = GetStdHandle(-12) };
            // Assign before resuming: even Node's first descendant belongs to our owned job.
            if (!CreateProcess(node, command, IntPtr.Zero, IntPtr.Zero, true, 0x08000004,
                IntPtr.Zero, null, ref startup, out child)) throw new InvalidOperationException();
            if (!AssignProcessToJobObject(job, child.process) || ResumeThread(child.thread) == UInt32.MaxValue)
                throw new InvalidOperationException();
            if (WaitForSingleObject(child.process, UInt32.MaxValue) != 0) throw new InvalidOperationException();
            uint code; if (!GetExitCodeProcess(child.process, out code)) throw new InvalidOperationException();
            return code <= Int32.MaxValue ? (int)code : 1;
        }
        catch { Console.Error.WriteLine("Observer proxy launcher failed; details suppressed."); return 1; }
        finally
        {
            if (child.process != IntPtr.Zero) { TerminateProcess(child.process, 1); CloseHandle(child.process); }
            if (child.thread != IntPtr.Zero) CloseHandle(child.thread);
            if (job != IntPtr.Zero) CloseHandle(job);
        }
    }
}
