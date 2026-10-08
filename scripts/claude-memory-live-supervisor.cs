// Owned test launcher for Bun.spawn([exe, "app-server", ...]); no service/install actions.
using System;
using System.Runtime.InteropServices;
using System.Text;
using System.IO;
using System.Text.RegularExpressions;
using System.Web.Script.Serialization;
using System.Diagnostics;
using System.Threading;
using System.Threading.Tasks;
using System.Collections.Generic;

internal static class LiveMemorySupervisor
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
    [StructLayout(LayoutKind.Sequential)] private struct Accounting
    { public long user, kernel, periodUser, periodKernel; public uint faults, total, active, terminated; }
    [DllImport("kernel32.dll", SetLastError = true)] private static extern bool TerminateJobObject(IntPtr job, uint code);
    [DllImport("kernel32.dll", SetLastError = true)] private static extern bool QueryInformationJobObject(IntPtr job, int kind, ref Accounting info, uint size, IntPtr returned);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)] private static extern bool CreateHardLink(string link, string original, IntPtr reserved);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)] private static extern IntPtr CreateFile(string name, uint access, uint sharing, IntPtr security, uint creation, uint flags, IntPtr template);
    [DllImport("kernel32.dll", SetLastError = true)] private static extern bool SetHandleInformation(IntPtr handle, uint mask, uint flags);
    [DllImport("kernel32.dll", SetLastError = true)] private static extern IntPtr OpenProcess(uint access, bool inherit, uint id);
    [DllImport("iphlpapi.dll")] private static extern uint GetExtendedTcpTable(IntPtr table, ref uint size, bool sort, uint family, uint kind, uint reserved);
    [DllImport("kernel32.dll", SetLastError = true)] private static extern bool IsProcessInJob(IntPtr process, IntPtr job, out bool member);
    private static bool PortOwned(IntPtr job, int port)
    {
        if (port < 1024 || port > 65535) return false;
        uint size = 0; GetExtendedTcpTable(IntPtr.Zero, ref size, false, 2, 5, 0);
        if (size < 4 || size > 1024 * 1024) return false;
        IntPtr table = Marshal.AllocHGlobal((int)size);
        try
        {
            if (GetExtendedTcpTable(table, ref size, false, 2, 5, 0) != 0) return false;
            int count = Marshal.ReadInt32(table); if (count < 0 || count > ((int)size - 4) / 24) return false;
            int matches = 0;
            for (int index = 0; index < count; index++)
            {
                IntPtr row = IntPtr.Add(table, 4 + index * 24);
                int encoded = Marshal.ReadInt32(row, 8), rowPort = ((encoded & 255) << 8) | ((encoded >> 8) & 255);
                if (Marshal.ReadInt32(row) != 2 || rowPort != port) continue;
                int address = Marshal.ReadInt32(row, 4); if (address != 0 && address != 0x0100007f) return false;
                IntPtr process = OpenProcess(0x00100000 | 0x1000, false, (uint)Marshal.ReadInt32(row, 20));
                if (process == IntPtr.Zero) return false;
                try { bool member; if (!IsProcessInJob(process, job, out member) || !member) return false; matches++; }
                finally { CloseHandle(process); }
            }
            return matches == 1;
        }
        finally { Marshal.FreeHGlobal(table); }
    }
    private static readonly JavaScriptSerializer Json = new JavaScriptSerializer { MaxJsonLength = 65536, RecursionLimit = 6 };
    private static readonly Stopwatch Clock = Stopwatch.StartNew();
    private static readonly List<ProcessInfo> Children = new List<ProcessInfo>();
    private static int Lifetime;
    private static int Remaining() { return Math.Max(0, Lifetime - (int)Clock.ElapsedMilliseconds); }
    private static void Emit(object record) { try { Console.WriteLine(Json.Serialize(record)); Console.Out.Flush(); } catch { /* Parent may have crashed; cleanup still runs. */ } }
    private static string PathChecked(string path)
    {
        if (String.IsNullOrEmpty(path) || path.Length > 2048 || Path.GetFullPath(path) != path) throw new InvalidOperationException();
        string cursor = path;
        while (!String.IsNullOrEmpty(cursor))
        {
            if (File.Exists(cursor) || Directory.Exists(cursor))
                if ((File.GetAttributes(cursor) & FileAttributes.ReparsePoint) != 0) throw new InvalidOperationException();
            cursor = Path.GetDirectoryName(cursor);
        }
        return path;
    }
    private static string Owned(string root, string path)
    {
        PathChecked(path);
        if (!path.StartsWith(root + Path.DirectorySeparatorChar, StringComparison.OrdinalIgnoreCase)) throw new InvalidOperationException();
        return path;
    }
    private static string ReadLineBounded(Stream input)
    {
        var bytes = new List<byte>(1024);
        while (bytes.Count <= 65536)
        {
            int b = input.ReadByte(); if (b == -1) { if (bytes.Count == 0) return null; throw new InvalidOperationException(); }
            if (b == 10) return new UTF8Encoding(false, true).GetString(bytes.ToArray());
            bytes.Add((byte)b);
        }
        throw new InvalidOperationException();
    }
    private static void Keys(Dictionary<string, object> value, params string[] keys)
    {
        if (value.Count != keys.Length) throw new InvalidOperationException();
        foreach (string key in keys) if (!value.ContainsKey(key)) throw new InvalidOperationException();
    }
    private static IntPtr OpenStdio(string path, uint access)
    {
        IntPtr handle = CreateFile(path, access, 3, IntPtr.Zero, 3, 0, IntPtr.Zero);
        if (handle == new IntPtr(-1) || !SetHandleInformation(handle, 1, 1)) throw new InvalidOperationException();
        return handle;
    }
    private static int Start(IntPtr job, string root, Dictionary<string, object> request)
    {
        Keys(request, "action", "binary", "args", "env", "cwd", "input");
        if (Children.Count >= 16 || Remaining() == 0) throw new InvalidOperationException();
        string binary = PathChecked((string)request["binary"]), cwd = Owned(root, (string)request["cwd"]);
        if (!File.Exists(binary) || Path.GetExtension(binary).ToLowerInvariant() != ".exe" || !Directory.Exists(cwd)) throw new InvalidOperationException();
        var command = new StringBuilder(Quote(binary));
        var arguments = (System.Collections.IList)request["args"];
        if (arguments.Count > 32) throw new InvalidOperationException();
        foreach (object argument in arguments) { string text = (string)argument; if (text.Length > 4096) throw new InvalidOperationException(); command.Append(' ').Append(Quote(text)); }
        var environment = (Dictionary<string, object>)request["env"]; var block = new StringBuilder();
        if (environment.Count > 80) throw new InvalidOperationException();
        var allowed = new HashSet<string>(StringComparer.OrdinalIgnoreCase) { "PATH", "PATHEXT", "SYSTEMROOT", "WINDIR", "TEMP", "TMP", "TMPDIR", "HOME", "USERPROFILE", "CODEX_HOME", "APPDATA", "LOCALAPPDATA", "CLAUDE_PLUGIN_ROOT", "PLUGIN_ROOT", "DO_NOT_TRACK" };
        var envKeys = new List<string>(environment.Keys); envKeys.Sort(StringComparer.OrdinalIgnoreCase);
        foreach (string key in envKeys)
        {
            var pair = new KeyValuePair<string, object>(key, environment[key]);
            string value = (string)pair.Value;
            if ((!allowed.Contains(pair.Key) && !pair.Key.StartsWith("CLAUDE_MEM_", StringComparison.Ordinal))
                || pair.Key.IndexOfAny(new[] { '\0', '=' }) >= 0 || value.IndexOf('\0') >= 0 || value.Length > 4096) throw new InvalidOperationException();
            block.Append(pair.Key).Append('=').Append(value).Append('\0');
        }
        block.Append('\0'); if (block.Length > 32768) throw new InvalidOperationException();
        IntPtr env = Marshal.StringToHGlobalUni(block.ToString()), input = IntPtr.Zero, output = IntPtr.Zero;
        var child = new ProcessInfo();
        try
        {
            string inputPath = request["input"] == null ? "NUL" : Owned(root, (string)request["input"]);
            input = OpenStdio(inputPath, 0x80000000); output = OpenStdio("NUL", 0x40000000);
            var startup = new StartupInfo { size = (uint)Marshal.SizeOf(typeof(StartupInfo)), flags = 0x100, input = input, output = output, error = output };
            if (!CreateProcess(binary, command, IntPtr.Zero, IntPtr.Zero, true, 0x08000404, env, cwd, ref startup, out child)) throw new InvalidOperationException();
            if (!AssignProcessToJobObject(job, child.process) || ResumeThread(child.thread) == UInt32.MaxValue) throw new InvalidOperationException();
            Children.Add(child); return Children.Count - 1;
        }
        catch { if (child.process != IntPtr.Zero) { TerminateProcess(child.process, 1); CloseHandle(child.process); } if (child.thread != IntPtr.Zero) CloseHandle(child.thread); throw; }
        finally { if (input != IntPtr.Zero) CloseHandle(input); if (output != IntPtr.Zero) CloseHandle(output); Marshal.FreeHGlobal(env); }
    }
    private static void CleanAuthAliases(string root, string preserve)
    {
        // Never follow reparse directories. Only aliases in this fresh owned root are removed.
        foreach (string directory in Directory.GetDirectories(root)) { PathChecked(directory); CleanAuthAliases(directory, preserve); }
        foreach (string file in Directory.GetFiles(root, "auth.json")) { Owned(root, file); if (!String.Equals(file, preserve, StringComparison.OrdinalIgnoreCase)) File.Delete(file); }
    }
    private static bool StopAndWait(IntPtr job)
    {
        if (job == IntPtr.Zero) return true;
        if (!TerminateJobObject(job, 1)) return false;
        var timer = Stopwatch.StartNew();
        while (timer.ElapsedMilliseconds < 10000)
        {
            var info = new Accounting();
            if (!QueryInformationJobObject(job, 1, ref info, (uint)Marshal.SizeOf(typeof(Accounting)), IntPtr.Zero)) return false;
            if (info.active == 0) return true;
            Thread.Sleep(10);
        }
        return false;
    }
    private static IntPtr NewJob()
    {
        IntPtr job = CreateJobObject(IntPtr.Zero, null); if (job == IntPtr.Zero) throw new InvalidOperationException();
        var limits = new ExtendedLimits(); limits.basic.flags = 0x2000;
        if (!SetInformationJobObject(job, 9, ref limits, (uint)Marshal.SizeOf(typeof(ExtendedLimits)))) { CloseHandle(job); throw new InvalidOperationException(); }
        return job;
    }
    private static string ReadConfig(string path)
    {
        using (var stream = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.Read))
        {
            if (stream.Length < 1 || stream.Length > 8192) throw new InvalidOperationException();
            var bytes = new byte[(int)stream.Length]; int offset = 0;
            while (offset < bytes.Length) { int count = stream.Read(bytes, offset, bytes.Length - offset); if (count == 0) throw new InvalidOperationException(); offset += count; }
            if (stream.ReadByte() != -1) throw new InvalidOperationException();
            return new UTF8Encoding(false, true).GetString(bytes);
        }
    }
    private static int Main(string[] args)
    {
        IntPtr job = IntPtr.Zero, owner = IntPtr.Zero; FileStream guard = null; string root = null; bool stopped = false;
        try
        {
            if (args.Length != 1) throw new InvalidOperationException();
            string sidecar = PathChecked(args[0]);
            var config = Json.Deserialize<Dictionary<string, object>>(ReadConfig(sidecar));
            Keys(config, "root", "authSource", "authLink", "lifetimeMs", "ownerPid");
            uint ownerId = Convert.ToUInt32(config["ownerPid"]);
            owner = OpenProcess(0x00100000, false, ownerId);
            if (owner == IntPtr.Zero || WaitForSingleObject(owner, 0) != 258) throw new InvalidOperationException();
            root = PathChecked((string)config["root"]);
            if (!Regex.IsMatch(Path.GetFileName(root), @"\Arun-[0-9a-f-]{36}\z") || !Directory.Exists(root)) throw new InvalidOperationException();
            Lifetime = Convert.ToInt32(config["lifetimeMs"]); if (Lifetime < 1 || Lifetime > 240000) throw new InvalidOperationException();
            string source = PathChecked((string)config["authSource"]), link = Owned(root, (string)config["authLink"]);
            if (source.StartsWith(root + Path.DirectorySeparatorChar, StringComparison.OrdinalIgnoreCase) || Path.GetFileName(link) != "auth.json") throw new InvalidOperationException();
            guard = new FileStream(source, FileMode.Open, FileAccess.Read, FileShare.Read); // No read of auth bytes.
            if (File.Exists(link) || !CreateHardLink(link, source, IntPtr.Zero)) throw new InvalidOperationException();
            job = NewJob();
            Emit(new { ready = true }); var stdin = Console.OpenStandardInput();
            for (int count = 0; count < 128; count++)
            {
                var line = Task.Run(() => ReadLineBounded(stdin));
                while (!line.Wait(Math.Min(50, Remaining())))
                {
                    if (WaitForSingleObject(owner, 0) == 0 || Remaining() == 0) break;
                }
                if (WaitForSingleObject(owner, 0) == 0) break;
                if (!line.IsCompleted || Remaining() == 0) throw new InvalidOperationException();
                string text = line.Result; if (text == null) break;
                var request = Json.Deserialize<Dictionary<string, object>>(text); string action = (string)request["action"];
                if (action == "start") Emit(new { started = true, process = Start(job, root, request) });
                else if (action == "status" || action == "wait")
                {
                    Keys(request, "action", "process"); int index = Convert.ToInt32(request["process"]);
                    if (index < 0 || index >= Children.Count) throw new InvalidOperationException();
                    if (action == "wait" && WaitForSingleObject(Children[index].process, (uint)Remaining()) != 0) throw new InvalidOperationException();
                    uint code; if (!GetExitCodeProcess(Children[index].process, out code)) throw new InvalidOperationException();
                    Emit(new { running = code == 259, success = code == 0 });
                }
                else if (action == "port") { Keys(request, "action", "port"); Emit(new { portOwned = PortOwned(job, Convert.ToInt32(request["port"])) }); }
                else if (action == "reset")
                {
                    Keys(request, "action"); if (!StopAndWait(job)) throw new InvalidOperationException();
                    CleanAuthAliases(root, link);
                    foreach (var child in Children) { CloseHandle(child.process); CloseHandle(child.thread); }
                    Children.Clear(); CloseHandle(job); job = IntPtr.Zero; job = NewJob();
                    Emit(new { reset = true });
                }
                else if (action == "stop") { Keys(request, "action"); break; }
                else throw new InvalidOperationException();
            }
            stopped = StopAndWait(job); if (!stopped) throw new InvalidOperationException();
            CleanAuthAliases(root, null); guard.Dispose(); guard = null; Emit(new { stopped = true }); return 0;
        }
        catch { Emit(new { failed = true }); return 1; }
        finally
        {
            if (!stopped) stopped = StopAndWait(job);
            if (stopped && root != null && guard != null) { try { CleanAuthAliases(root, null); } catch {} }
            foreach (var child in Children) { CloseHandle(child.process); CloseHandle(child.thread); }
            if (job != IntPtr.Zero) CloseHandle(job);
            if (owner != IntPtr.Zero) CloseHandle(owner);
            // Forced termination of this coordinator itself is NOT a proven auth-order barrier.
            if (guard != null) guard.Dispose();
        }
    }
}
