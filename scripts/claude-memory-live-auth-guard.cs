// Fixture only: retain a read/share-read handle without reading any file bytes.
using System;
using System.IO;

internal static class LiveAuthReadGuard
{
    private static int Main(string[] args)
    {
        if (args.Length != 1) { Console.Out.WriteLine("{\"ready\":false}"); return 1; }
        try
        {
            using (var handle = new FileStream(args[0], FileMode.Open, FileAccess.Read, FileShare.Read))
            {
                Console.Out.WriteLine("{\"ready\":true}"); Console.Out.Flush();
                // EOF or one control byte releases the guard. The parent must first
                // await termination of every owned worker/observer descendant.
                Console.OpenStandardInput().ReadByte();
            }
            return 0;
        }
        catch { Console.Out.WriteLine("{\"ready\":false}"); return 1; }
    }
}
