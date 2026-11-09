using System;
using System.Collections.Generic;

internal static class NativeDeveloperCacheRegression
{
    private static int Main(string[] args)
    {
        try
        {
            if (args[0] == "files") NativeDeveloperCache.VerifyFiles(args[1], NativeDeveloperCache.Json(args[2]));
            else if (args[0] == "verify") NativeDeveloperCache.Verify(args[1]);
            else throw new ArgumentException("Unknown cache regression operation");
            return 0;
        }
        catch (Exception error)
        {
            Console.Error.WriteLine(error.Message);
            return 2;
        }
    }
}
