using System;
using System.Collections.Generic;
using System.IO;
using System.Security.Cryptography;
using System.Text.RegularExpressions;
using System.Web.Script.Serialization;

internal static class NativeDeveloperCache
{
    internal static Dictionary<string, object> Json(string path)
    {
        return new JavaScriptSerializer().Deserialize<Dictionary<string, object>>(File.ReadAllText(path));
    }

    internal static string Digest(string path)
    {
        using (var stream = File.OpenRead(path))
        using (var sha = SHA256.Create())
            return BitConverter.ToString(sha.ComputeHash(stream)).Replace("-", "").ToLowerInvariant();
    }

    private static string Constant(string source, string name)
    {
        var matches = Regex.Matches(source, @"const " + Regex.Escape(name) + @" = '([^']+)';");
        if (matches.Count != 1) throw new InvalidDataException("Missing or ambiguous preparation pin: " + name);
        return matches[0].Groups[1].Value;
    }

    private static Dictionary<string, object> Files(string source)
    {
        var blocks = Regex.Matches(source, @"const pinnedFiles = \{([^}]+)\};");
        if (blocks.Count != 1) throw new InvalidDataException("Missing preparation file pins");
        var result = new Dictionary<string, object>(StringComparer.Ordinal);
        foreach (Match match in Regex.Matches(blocks[0].Groups[1].Value, @"'([^']+)': '([a-f0-9]{64})'"))
            result.Add(match.Groups[1].Value, match.Groups[2].Value);
        if (result.Count == 0) throw new InvalidDataException("Empty preparation file pins");
        return result;
    }

    private static void SameFiles(Dictionary<string, object> expected, Dictionary<string, object> actual)
    {
        if (actual.Count != expected.Count) throw new InvalidDataException("Cache file pin count differs");
        foreach (var pair in expected)
            if (!actual.ContainsKey(pair.Key) || !Object.Equals(actual[pair.Key], pair.Value))
                throw new InvalidDataException("Cache file pin differs: " + pair.Key);
    }

    internal static void VerifyFiles(string directory, Dictionary<string, object> files)
    {
        foreach (var pair in files)
        {
            var hash = pair.Value as string;
            if (hash == null || !Regex.IsMatch(hash, @"\A[a-f0-9]{64}\z") || hash == new string('0', 64))
                throw new InvalidDataException("Invalid SHA256 pin: " + pair.Key);
            var path = Path.GetFullPath(Path.Combine(directory, pair.Key.Replace('/', Path.DirectorySeparatorChar)));
            if (!path.StartsWith(Path.GetFullPath(directory) + Path.DirectorySeparatorChar, StringComparison.OrdinalIgnoreCase))
                throw new InvalidDataException("Cache pin escapes directory");
            if (!File.Exists(path) || Digest(path) != hash)
                throw new InvalidDataException("Missing or changed pinned cache file: " + path);
        }
    }

    internal static void Verify(string root)
    {
        var policy = Json(Path.Combine(root, "scripts/native/packaging/policy.json"));
        var cache = Path.Combine(root, "rust/target");
        foreach (var name in new[] { "onnx", "libclang" })
        {
            var source = File.ReadAllText(Path.Combine(root, "scripts/native/prepare-" + name + ".cjs"));
            var version = Constant(source, "version");
            if (version != (name == "onnx" ? "1.31.1" : "18.1.1")) throw new InvalidDataException("Preparation version changed");
            var files = Files(source);
            if (files.Count != (name == "onnx" ? 6 : 2)) throw new InvalidDataException("Preparation file pin count changed");
            SameFiles(files, (Dictionary<string, object>)policy[name == "onnx" ? "onnxFiles" : "libclangFiles"]);
            var directory = Path.Combine(cache, (name == "onnx" ? "onnxruntime" : name) + "-win-x64-" + version);
            var manifest = Json(Path.Combine(directory, "manifest.json"));
            var pin = name == "onnx" ? "packageHash" : "archiveHash";
            if (!Object.Equals(manifest["version"], version) || !Object.Equals(manifest[pin], Constant(source, pin)))
                throw new InvalidDataException("Cache manifest identity differs: " + name);
            SameFiles(files, (Dictionary<string, object>)manifest["files"]);
            VerifyFiles(directory, files);
            var actual = Directory.GetFiles(directory, "*", SearchOption.AllDirectories);
            if (actual.Length != files.Count + 1) throw new InvalidDataException("Unexpected unverified cache files: " + directory);
        }
        var rnnoise = File.ReadAllText(Path.Combine(root, "scripts/native/prepare-rnnoise.cjs"));
        var rnnoiseHash = Constant(rnnoise, "expected");
        if (rnnoiseHash != "4ac81c5c0884ec4bd5907026aaae16209b7b76cd9d7f71af582094a2f98f4b43")
            throw new InvalidDataException("RNNoise pin changed");
        VerifyFiles(Path.Combine(cache, "rnnoise-4ac81c5c"), new Dictionary<string, object> { { "rnnoise-modeltar.gz", rnnoiseHash } });
        if (!Object.Equals(policy["webRtcRevision"], "a545de4")) throw new InvalidDataException("WebRTC revision changed");
        VerifyFiles(Path.Combine(cache, "webrtc-win-x64-a545de4"), new Dictionary<string, object> {
            { "lib/webrtc.lib", policy["webRtcLibrarySha256"] }, { "LICENSE.md", policy["webRtcLicenseSha256"] }
        });
    }
}
