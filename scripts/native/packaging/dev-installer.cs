using System;
using System.Diagnostics;
using System.IO;
using System.IO.Compression;
using System.Reflection;
using System.Windows.Forms;
using Microsoft.Win32;

static class Program
{
    const string Product = "zabor (developer preview)";
    const string Version = "0.1.0-dev";
    const string KeyName = @"Software\Microsoft\Windows\CurrentVersion\Uninstall\ZaborDev";
    const string ResourceName = "payload.zip";

    [STAThread]
    static int Main(string[] args)
    {
        var quiet = Array.IndexOf(args, "/quiet") >= 0;
        try
        {
            if (args.Length == 1 && args[0] == "--uninstall")
            {
                return Uninstall(quiet);
            }
            if (args.Length == 2 && args[0] == "--uninstall" && args[1] == "/quiet")
            {
                return Uninstall(true);
            }
            if (args.Length == 1 && args[0] == "/quiet")
            {
                return Install(true);
            }
            if (args.Length != 0)
            {
                return Fail("unsupported arguments");
            }
            return Install(quiet);
        }
        catch (Exception error)
        {
            return Fail(error.Message);
        }
    }

    static string Root()
    {
        return Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "ZaborDev");
    }

    static void RequireStopped()
    {
        foreach (var process in Process.GetProcessesByName("zabor-native"))
        {
            throw new InvalidOperationException("close zabor (developer preview) before installing or removing it");
        }
    }

    static int Install(bool quiet)
    {
        RequireStopped();
        var root = Root();
        var app = Path.Combine(root, "app");
        if (Directory.Exists(app))
        {
            Directory.Delete(app, true);
        }
        Directory.CreateDirectory(app);
        using (var payload = Assembly.GetExecutingAssembly().GetManifestResourceStream(ResourceName))
        {
            if (payload == null)
            {
                throw new InvalidOperationException("installer payload is missing");
            }
            using (var archive = new ZipArchive(payload, ZipArchiveMode.Read))
            {
                foreach (var entry in archive.Entries)
                {
                    var destination = Path.GetFullPath(Path.Combine(app, entry.FullName));
                    if (!destination.StartsWith(Path.GetFullPath(app) + Path.DirectorySeparatorChar, StringComparison.OrdinalIgnoreCase))
                    {
                        throw new InvalidOperationException("payload entry escapes the install directory");
                    }
                    if (entry.FullName.EndsWith("/", StringComparison.Ordinal))
                    {
                        Directory.CreateDirectory(destination);
                        continue;
                    }
                    Directory.CreateDirectory(Path.GetDirectoryName(destination));
                    entry.ExtractToFile(destination, true);
                }
            }
        }
        var exe = Path.Combine(app, "zabor-native.exe");
        if (!File.Exists(exe))
        {
            throw new InvalidOperationException("payload does not contain zabor-native.exe");
        }
        Shortcut(Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.DesktopDirectory), "zabor (dev).lnk"), exe, app);
        var programs = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.Programs), "zabor (dev)");
        Directory.CreateDirectory(programs);
        Shortcut(Path.Combine(programs, "zabor (dev).lnk"), exe, app);
        Shortcut(Path.Combine(programs, "uninstall zabor (dev).lnk"), Assembly.GetExecutingAssembly().Location, app, "--uninstall");
        using (var key = Registry.CurrentUser.CreateSubKey(KeyName))
        {
            key.SetValue("DisplayName", Product);
            key.SetValue("DisplayVersion", Version);
            key.SetValue("Publisher", "zabor developer build");
            key.SetValue("InstallLocation", root);
            key.SetValue("UninstallString", "\"" + Assembly.GetExecutingAssembly().Location + "\" --uninstall");
            key.SetValue("NoModify", 1, RegistryValueKind.DWord);
            key.SetValue("NoRepair", 1, RegistryValueKind.DWord);
        }
        if (!quiet)
        {
            MessageBox.Show(
                "Installed to " + root + Environment.NewLine +
                "Shortcut: zabor (dev). This build bypasses installation trust and uses the zabor-desktop-native-dev profile.",
                Product,
                MessageBoxButtons.OK,
                MessageBoxIcon.Information);
        }
        return 0;
    }

    static int Uninstall(bool quiet)
    {
        RequireStopped();
        var root = Root();
        SafeDelete(Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.DesktopDirectory), "zabor (dev).lnk"));
        SafeDelete(Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.Programs), "zabor (dev)"));
        Registry.CurrentUser.DeleteSubKeyTree(KeyName, false);
        if (Directory.Exists(root))
        {
            Directory.Delete(root, true);
        }
        if (!quiet)
        {
            MessageBox.Show(
                "Removed " + root + ". The zabor-desktop-native-dev profile in APPDATA is kept.",
                Product,
                MessageBoxButtons.OK,
                MessageBoxIcon.Information);
        }
        return 0;
    }

    static void SafeDelete(string path)
    {
        if (Directory.Exists(path))
        {
            Directory.Delete(path, true);
        }
        else if (File.Exists(path))
        {
            File.Delete(path);
        }
    }

    static void Shortcut(string path, string target, string workingDirectory, string arguments = "--developer")
    {
        var shell = Activator.CreateInstance(Type.GetTypeFromProgID("WScript.Shell"));
        var shortcut = shell.GetType().InvokeMember("CreateShortcut", BindingFlags.InvokeMethod, null, shell, new object[] { path });
        var type = shortcut.GetType();
        type.InvokeMember("TargetPath", BindingFlags.SetProperty, null, shortcut, new object[] { target });
        type.InvokeMember("Arguments", BindingFlags.SetProperty, null, shortcut, new object[] { arguments });
        type.InvokeMember("WorkingDirectory", BindingFlags.SetProperty, null, shortcut, new object[] { workingDirectory });
        type.InvokeMember("IconLocation", BindingFlags.SetProperty, null, shortcut, new object[] { target });
        type.InvokeMember("Description", BindingFlags.SetProperty, null, shortcut, new object[] { Product });
        type.InvokeMember("Save", BindingFlags.InvokeMethod, null, shortcut, null);
    }

    static int Fail(string message)
    {
        MessageBox.Show(message, Product, MessageBoxButtons.OK, MessageBoxIcon.Error);
        return 1;
    }}
