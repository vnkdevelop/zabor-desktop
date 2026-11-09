using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Management;
using System.Text;
using System.Text.RegularExpressions;
using System.Web.Script.Serialization;

internal static class NativeDeveloperLauncher
{
    private const string Target = "x86_64-pc-windows-msvc";

    private static string[] BatchArguments(string root)
    {
        uint parent;
        using (var current = new ManagementObject("Win32_Process.Handle='" + Process.GetCurrentProcess().Id + "'"))
        {
            current.Get();
            parent = (uint)current["ParentProcessId"];
        }
        var cmd = Path.Combine(Environment.SystemDirectory, "cmd.exe");
        using (var wrapper = new ManagementObject("Win32_Process.Handle='" + parent + "'"))
        {
            wrapper.Get();
            if (!String.Equals((string)wrapper["ExecutablePath"], cmd, StringComparison.OrdinalIgnoreCase))
                throw new InvalidOperationException("Batch transport requires an explicit cmd.exe /c invocation");
            parent = (uint)wrapper["ParentProcessId"];
        }
        string statement;
        string options;
        using (var caller = new ManagementObject("Win32_Process.Handle='" + parent + "'"))
        {
            caller.Get();
            if (!String.Equals((string)caller["ExecutablePath"], cmd, StringComparison.OrdinalIgnoreCase))
                throw new InvalidOperationException("Batch transport requires an explicit cmd.exe /c caller");
            var command = (string)caller["CommandLine"];
            var match = Regex.Match(command ?? "", @"\A(.*?)\s/c(?:\s+|(?=""))(.+)\z", RegexOptions.IgnoreCase | RegexOptions.Singleline);
            if (!match.Success) throw new InvalidOperationException("Interactive/batch-chain transport is unsupported; use an explicit fixed cmd.exe /c selector");
            options = match.Groups[1].Value;
            statement = match.Groups[2].Value.Trim();
        }
        if (statement.StartsWith("\"", StringComparison.Ordinal))
        {
            int last = statement.LastIndexOf('"');
            if (last == 0) throw new ArgumentException("Unbalanced raw CMD syntax");
            string inside = statement.Substring(1, last - 1);
            bool preserve = !Regex.IsMatch(options, @"(?:\A|\s)/s(?:\s|\z)", RegexOptions.IgnoreCase)
                && statement.Count(value => value == '"') == 2
                && inside.Any(Char.IsWhiteSpace) && inside.IndexOfAny("&<>()@^|".ToCharArray()) < 0
                && File.Exists(Path.GetFullPath(Path.Combine(root, inside)));
            if (!preserve) statement = inside + statement.Substring(last + 1);
        }
        if (statement.Length + cmd.Length + 24 > 8191)
            throw new ArgumentException("Raw selector command exceeds the supported CMD 8191-character limit");
        var values = CmdSelectors(statement);
        if (values.Length == 0 || !String.Equals(Path.GetFullPath(Path.Combine(root, values[0])), Path.Combine(root, "native.bat"), StringComparison.OrdinalIgnoreCase))
            throw new InvalidOperationException("Only a single explicit native.bat command is supported; no shell or batch-chain fallback");
        var selector = values.Skip(1).Select(value => value.ToLowerInvariant()).ToArray();
        var allowed = new[] { "run", "build", "check", "help", "fmt", "clippy", "test", "cache-check", "prepare", "reference-check", "dist", "dist:win", "publish", "release", "release:beta", "data" };
        bool fixedCall = selector.Length == 0 || (selector.Length == 1 && allowed.Contains(selector[0]))
            || selector.SequenceEqual(new[] { "build", "--release" }) || selector.SequenceEqual(new[] { "run", "--scale", "2" });
        if (!fixedCall) throw new ArgumentException("Raw runtime argv is unsupported; use fixed selector native.bat data with JSON argv-as-data");
        var environment = Environment.GetEnvironmentVariables();
        var jsonKey = environment.Keys.Cast<string>().SingleOrDefault(key => String.Equals(key, "ZABOR_NATIVE_ARGV_JSON", StringComparison.OrdinalIgnoreCase));
        var fileKey = environment.Keys.Cast<string>().SingleOrDefault(key => String.Equals(key, "ZABOR_NATIVE_ARGV_FILE", StringComparison.OrdinalIgnoreCase));
        bool data = selector.SequenceEqual(new[] { "data" });
        if (!data)
        {
            if (jsonKey != null || fileKey != null) throw new ArgumentException("JSON argv-as-data requires the fixed native.bat data selector; requests are never silently ignored");
            return selector;
        }
        if ((jsonKey == null) == (fileKey == null)) throw new ArgumentException("The data selector requires exactly one ZABOR_NATIVE_ARGV_JSON or ZABOR_NATIVE_ARGV_FILE request");
        string payload = jsonKey != null ? (string)environment[jsonKey] : ReadRequestFile(Path.GetFullPath(Path.Combine(root, (string)environment[fileKey])));
        if (payload.Length > 8191) throw new ArgumentException("JSON argv-as-data exceeds the supported 8191-character limit");
        var arguments = new JavaScriptSerializer().DeserializeObject(payload) as object[];
        if (arguments == null || arguments.Any(value => !(value is string))) throw new ArgumentException("JSON argv-as-data must be an array of strings");
        return arguments.Cast<string>().ToArray();
    }

    private static string ReadRequestFile(string path)
    {
        using (var reader = new StreamReader(path, Encoding.UTF8, true))
        {
            var buffer = new char[8192];
            int length = 0;
            while (length < buffer.Length)
            {
                int read = reader.Read(buffer, length, buffer.Length - length);
                if (read == 0) return new string(buffer, 0, length);
                length += read;
            }
        }
        throw new ArgumentException("JSON argv-as-data exceeds the supported 8191-character limit");
    }

    private static string[] CmdSelectors(string statement)
    {
        var result = new List<string>();
        var value = new StringBuilder();
        bool quoted = false;
        bool started = false;
        foreach (char character in statement)
        {
            if (character == '"') { quoted = !quoted; started = true; continue; }
            if (character == '\0' || character == '\r' || character == '\n' || (!quoted && "&|<>()".IndexOf(character) >= 0))
                throw new ArgumentException("Raw CMD operator chains are unsupported; caller-owned queued commands cannot be undone by this child");
            if (!quoted && character == '^') throw new ArgumentException("Caret escapes are unsupported raw CMD syntax; use JSON argv-as-data");
            if (!quoted && Char.IsWhiteSpace(character))
            {
                if (started) result.Add(value.ToString());
                value.Clear();
                started = false;
                continue;
            }
            value.Append(character);
            started = true;
        }
        if (quoted) throw new ArgumentException("Unbalanced raw CMD syntax; use JSON argv-as-data");
        if (started) result.Add(value.ToString());
        return result.ToArray();
    }

    internal static string Quote(string value)
    {
        if (value.IndexOfAny(new[] { '\0', '\r', '\n' }) >= 0) throw new ArgumentException("Control characters are not valid command arguments");
        var result = new StringBuilder("\"");
        int slashes = 0;
        foreach (char c in value)
        {
            if (c == '\\') { slashes++; continue; }
            result.Append('\\', c == '"' ? slashes * 2 + 1 : slashes);
            result.Append(c);
            slashes = 0;
        }
        result.Append('\\', slashes * 2);
        return result.Append('"').ToString();
    }

    internal static int Run(string root, string executable, IEnumerable<string> arguments, Dictionary<string, string> environment)
    {
        if (!File.Exists(executable) || !executable.EndsWith(".exe", StringComparison.OrdinalIgnoreCase))
            throw new FileNotFoundException("Developer executable not found", executable);
        var encoded = String.Join(" ", arguments.Select(Quote));
        var cmd = Path.Combine(Environment.SystemDirectory, "cmd.exe");
        var expanded = Quote(cmd) + " /d /v:on /s /c \"" + Quote(executable) + " " + encoded + "\"";
        if (expanded.Length > 8191) throw new ArgumentException("Expanded command exceeds the supported CMD 8191-character limit");
        var start = new ProcessStartInfo(cmd,
            "/d /v:on /s /c \"\"!ZABOR_DEVELOPER_EXE!\" !ZABOR_DEVELOPER_ARGS!\"");
        start.UseShellExecute = false;
        start.WorkingDirectory = root;
        foreach (var pair in environment) start.EnvironmentVariables[pair.Key] = pair.Value;
        start.EnvironmentVariables["ZABOR_DEVELOPER_EXE"] = executable;
        start.EnvironmentVariables["ZABOR_DEVELOPER_ARGS"] = encoded;
        foreach (string name in start.EnvironmentVariables.Keys)
            if (start.EnvironmentVariables[name].Length > 8191)
                throw new ArgumentException("Environment value exceeds the supported CMD 8191-character limit: " + name);
        using (var process = Process.Start(start))
        {
            process.WaitForExit();
            return process.ExitCode;
        }
    }

    private static string Tool(string name)
    {
        if (name == "cargo")
        {
            var home = Environment.GetEnvironmentVariable("CARGO_HOME");
            if (String.IsNullOrEmpty(home)) home = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), ".cargo");
            var cargo = Path.Combine(home, "bin/cargo.exe");
            if (File.Exists(cargo)) return Path.GetFullPath(cargo);
        }
        foreach (var directory in (Environment.GetEnvironmentVariable("PATH") ?? "").Split(';'))
        {
            if (String.IsNullOrWhiteSpace(directory)) continue;
            var candidate = Path.Combine(directory.Trim('"'), name + ".exe");
            if (File.Exists(candidate)) return Path.GetFullPath(candidate);
        }
        throw new FileNotFoundException("Install the explicit developer tool and retry: " + name + ".exe");
    }

    private static IEnumerable<string> Cargo(string command, bool release)
    {
        var args = new List<string> { command, "--manifest-path", "rust/Cargo.toml", "-p", "zabor-native" };
        if (command == "fmt") { args.AddRange(new[] { "--", "--check" }); return args; }
        args.AddRange(new[] { "--locked", "--offline", "--target", Target });
        if (command == "test" || command == "clippy") args.AddRange(new[] { "--features", "camera-test-source,stream-audio-test-source" });
        if (command == "clippy") args.AddRange(new[] { "--all-targets", "--", "-D", "warnings" });
        if (command == "test") args.AddRange(new[] { "--lib", "--bins" });
        if (release) args.Add("--release");
        return args;
    }

    private static int Main(string[] args)
    {
        try
        {
            if (args.Length == 0) throw new ArgumentException("Use native.bat from the repository");
            if (Environment.OSVersion.Platform != PlatformID.Win32NT || !Environment.Is64BitProcess)
                throw new InvalidOperationException("Native developer commands require Windows x64");
            var root = Path.GetFullPath(args[0]);
            var forwarded = args.Length == 2 && args[1] == "--batch" ? BatchArguments(root) : args.Skip(1).ToArray();
            var command = forwarded.Length == 0 ? "run" : forwarded[0];
            if (new[] { "dist", "dist:win", "publish", "release", "release:beta" }.Contains(command))
                throw new InvalidOperationException("Packaging/publishing is blocked. Native developer builds are not certified end-user packages. See docs/migration/native-packaging.md.");
            if (command == "help")
            {
                Console.WriteLine("native.bat [run | build [--release] | check | fmt | clippy | test | cache-check | prepare | reference-check | data]");
                Console.WriteLine("Developer tools only. Run/build/check verify existing caches and never download. Reference checks require explicit Node/Electron/Python tooling.");
                Console.WriteLine("Untrusted/runtime argv must be a JSON string array in ZABOR_NATIVE_ARGV_JSON or ZABOR_NATIVE_ARGV_FILE, supplied before the fixed native.bat data call. This child cannot stop commands already parsed by its caller.");
                return 0;
            }
            var environment = new Dictionary<string, string>();
            if (command == "prepare")
            {
                if (forwarded.Length != 1) throw new ArgumentException("prepare accepts no arguments");
                var node = Tool("node");
                foreach (var name in new[] { "onnx", "libclang", "rnnoise" })
                {
                    int status = Run(root, node, new[] { "scripts/native/prepare-" + name + ".cjs" }, environment);
                    if (status != 0) return status;
                }
                NativeDeveloperCache.Verify(root);
                return 0;
            }
            if (command == "reference-check")
                return Run(root, Tool("node"), new[] { "scripts/native/verify.cjs" }.Concat(forwarded.Skip(1)), environment);
            NativeDeveloperCache.Verify(root);
            if (command == "cache-check")
            {
                if (forwarded.Length != 1) throw new ArgumentException("cache-check accepts no arguments");
                Console.WriteLine("Pinned ONNX 1.31.1, libclang 18.1.1, RNNoise and WebRTC a545de4 caches verified read-only.");
                return 0;
            }
            foreach (var variable in new[] { "RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_BUILD_RUSTFLAGS", "CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS" })
                if (Environment.GetEnvironmentVariables().Keys.Cast<string>().Any(key => String.Equals(key, variable, StringComparison.OrdinalIgnoreCase)))
                    throw new InvalidOperationException("Remove " + variable + " so the pinned MSVC/static-CRT configuration remains effective");
            var cache = Path.GetFullPath(Path.Combine(root, "rust/target"));
            environment["LK_CUSTOM_WEBRTC"] = Path.Combine(cache, "webrtc-win-x64-a545de4");
            environment["ORT_LIB_LOCATION"] = Path.GetFullPath(Path.Combine(cache, "onnxruntime-win-x64-1.31.1/native"));
            environment["ORT_PREFER_DYNAMIC_LINK"] = "1";
            environment["LIBCLANG_PATH"] = Path.GetFullPath(Path.Combine(cache, "libclang-win-x64-18.1.1/native"));
            environment["RNNOISE_MODEL_PATH"] = Path.GetFullPath(Path.Combine(cache, "rnnoise-4ac81c5c/rnnoise-modeltar.gz"));
            environment["CARGO_TARGET_DIR"] = Path.GetFullPath(Path.Combine(root, "rust/target/native-developer"));
            var cargo = Tool("cargo");
            if (command == "check")
            {
                if (forwarded.Length != 1) throw new ArgumentException("check accepts no arguments; use reference-check for parity");
                foreach (var step in new[] { "fmt", "clippy", "test", "build" })
                {
                    int status = Run(root, cargo, Cargo(step, false), environment);
                    if (status != 0) return status;
                }
                return 0;
            }
            if (new[] { "build", "fmt", "clippy", "test" }.Contains(command))
            {
                bool release = command == "build" && forwarded.Length == 2 && forwarded[1] == "--release";
                if (forwarded.Length != 1 && !release) throw new ArgumentException("Only build --release accepts a Cargo option; runtime arguments follow run");
                return Run(root, cargo, Cargo(command, release), environment);
            }
            var runtime = command == "run" || command == "--" ? forwarded.Skip(1) : forwarded;
            return Run(root, cargo, Cargo("run", false).Concat(new[] { "--" }).Concat(runtime), environment);
        }
        catch (Exception error)
        {
            Console.Error.WriteLine(error.Message);
            Console.Error.WriteLine("Missing caches: run native.bat prepare explicitly (Node developer tool only), restore the vetted WebRTC a545de4 SDK to rust/target/webrtc-win-x64-a545de4, and fetch locked Cargo dependencies explicitly. No automatic downloads or legacy fallback.");
            return 2;
        }
    }
}
