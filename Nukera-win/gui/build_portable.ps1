# Builds the single-file portable Nukera.exe
# Uses: Flutter build (gui), 7-Zip for payload compression, .NET csc for launcher.
$ErrorActionPreference = "Stop"
$RootDir     = Split-Path -Parent $PSScriptRoot
$GuiDir      = Join-Path $RootDir "gui"
$Icon        = Join-Path $GuiDir "windows\runner\resources\app_icon.ico"
$SevenZipExe = "C:\Program Files\7-Zip\7z.exe"
$CscExe      = "C:\Windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe"
$Work        = Join-Path $env:TEMP "nukera_build"
$Stage       = Join-Path $Work "payload\Nukera"
$PayloadZip  = Join-Path $Work "nukera_payload.zip"
$LauncherCs  = Join-Path $Work "nukera_launcher.cs"
$OutExe      = Join-Path $RootDir "Nukera.exe"

Write-Host "==> flutter build windows --release"
Push-Location $GuiDir
& "C:\flutter\bin\flutter.bat" build windows --release
if ($LASTEXITCODE -ne 0) { throw "flutter build failed" }
Pop-Location

Write-Host "==> staging lean payload"
if (Test-Path $Work) { Remove-Item -LiteralPath $Work -Recurse -Force }
New-Item -ItemType Directory -Force -Path "$Stage\gui","$Stage\cli","$Stage\engine","$Stage\tools","$Stage\state\jobs" | Out-Null

Copy-Item -Path "$GuiDir\build\windows\x64\runner\Release\*" -Destination "$Stage\gui" -Recurse -Force
Get-ChildItem -LiteralPath "$RootDir\cli" -File -Filter *.py | Copy-Item -Destination "$Stage\cli"
Copy-Item -LiteralPath "$RootDir\engine\bin"        -Destination "$Stage\engine\bin" -Recurse -Force
Copy-Item -LiteralPath "$RootDir\engine\.service"   -Destination "$Stage\engine\.service" -Recurse -Force
Copy-Item -LiteralPath "$RootDir\engine\strategies" -Destination "$Stage\engine\strategies" -Recurse -Force
Copy-Item -LiteralPath "$RootDir\engine\telegram-ws-proxy" -Destination "$Stage\engine\telegram-ws-proxy" -Recurse -Force
Copy-Item -LiteralPath "$RootDir\engine\utils"      -Destination "$Stage\engine\utils" -Recurse -Force
New-Item -ItemType Directory -Path "$Stage\engine\lists" | Out-Null
Get-ChildItem -LiteralPath "$RootDir\engine\lists" -File | Where-Object { $_.Name -notmatch '\.backup$' } | Copy-Item -Destination "$Stage\engine\lists"
Copy-Item -LiteralPath "$RootDir\engine\service.bat" -Destination "$Stage\engine" -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path "$Stage\tools\python" | Out-Null
Get-ChildItem -LiteralPath "$RootDir\tools\python" -Force | Where-Object { $_.Name -notmatch '__pycache__|\.cat$' } | ForEach-Object {
    Copy-Item -LiteralPath $_.FullName -Destination "$Stage\tools\python" -Recurse -Force
}
Get-ChildItem -LiteralPath $Stage -Recurse -Force -Directory -Filter __pycache__ | Remove-Item -Recurse -Force

Write-Host "==> compressing payload ($((Get-ChildItem $Stage -Recurse -File | Measure-Object Length -Sum).Sum/1MB) MB raw)"
New-Item -ItemType Directory -Force -Path $Work | Out-Null
& $SevenZipExe a -tzip -mx=9 $PayloadZip "$Stage\*" -bso0 -bsp0
if ($LASTEXITCODE -ne 0) { throw "7z failed" }
Write-Host "    payload $((Get-Item $PayloadZip).Length/1MB) MB"

Write-Host "==> compiling launcher"
$cs = @'
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.IO.Compression;
using System.Linq;
using System.Management;
using System.Reflection;
using System.Threading;

class NukeraLauncher {
    static readonly string Version = "1.0.0";
    static string TargetDir() {
        return Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Nukera");
    }
    static string LogFile() { return Path.Combine(Path.GetTempPath(), "nukera_launcher.log"); }
    static void Log(string m) { try { File.AppendAllText(LogFile(), DateTime.Now.ToString("HH:mm:ss ") + m + Environment.NewLine); } catch { } }

    static void KillTargetProcesses(string target) {
        // The resident elevated worker keeps the python DLLs loaded and cannot be
        // killed from a non-elevated launcher. Ask it to exit gracefully via its
        // jobs dir (state\jobs\shutdown.req); it deletes the req itself and exits.
        try {
            foreach (var p in Process.GetProcessesByName("nukera_gui")) { try { p.Kill(); p.WaitForExit(3000); } catch { } }
        } catch { }
        try {
            string jobs = Path.Combine(target, "state", "jobs");
            if (Directory.Exists(jobs) && File.Exists(Path.Combine(target, "state", "worker.pid"))) {
                string req = Path.Combine(jobs, "shutdown.req");
                File.WriteAllText(req, "shutdown");
                for (int i = 0; i < 40 && File.Exists(req); i++) System.Threading.Thread.Sleep(250);
            }
        } catch { }
        System.Threading.Thread.Sleep(500);
    }

    static void ExtractPayload(string target) {
        using (var s = Assembly.GetExecutingAssembly().GetManifestResourceStream("Nukera.Payload")) {
            if (s == null) throw new Exception("embedded payload missing");
            using (var zip = new ZipArchive(s, ZipArchiveMode.Read)) {
                foreach (var e in zip.Entries) {
                    string rel = e.FullName.Replace('\\', '/').TrimStart('/');
                    if (rel.Length == 0) continue;
                    if (e.Name.Length == 0 || e.FullName.EndsWith("/")) {
                        Directory.CreateDirectory(Path.Combine(target, rel.Replace('/', Path.DirectorySeparatorChar)));
                        continue;
                    }
                    string dest = Path.Combine(target, rel.Replace('/', Path.DirectorySeparatorChar));
                    Directory.CreateDirectory(Path.GetDirectoryName(dest));
                    using (var o = new FileStream(dest, FileMode.Create, FileAccess.Write))
                    using (var i = e.Open()) i.CopyTo(o);
                }
            }
        }
    }

    static void RunGui(string target) {
        string exe = Path.Combine(target, "gui", "nukera_gui.exe");
        if (!File.Exists(exe)) throw new Exception("nukera_gui.exe missing at " + exe);
        Process.Start(new ProcessStartInfo(exe) { WorkingDirectory = Path.GetDirectoryName(exe), UseShellExecute = false });
    }

    static int Main(string[] args) {
        try {
            string target = TargetDir();
            string marker = Path.Combine(target, ".nukera");
            bool force = args.Length > 0 && args[0] == "--force-extract";
            bool upToDate = File.Exists(marker) && File.ReadAllText(marker).Trim() == Version;
            if (force || !upToDate) {
                Log("extracting v" + Version + " to " + target);
                KillTargetProcesses(target);
                if (Directory.Exists(target)) {
                    try {
                        var p = Process.Start(new ProcessStartInfo("cmd.exe",
                            "/c rmdir /s /q \"" + target + "\"") {
                            CreateNoWindow = true, UseShellExecute = false
                        });
                        if (p != null) p.WaitForExit(5000);
                    } catch {}
                    if (Directory.Exists(target)) {
                        try { Directory.Delete(target, true); } catch {}
                    }
                }
                Directory.CreateDirectory(target);
                ExtractPayload(target);
                File.WriteAllText(marker, Version);
                Log("done");
            } else {
                Log("up to date, launching");
            }
            RunGui(target);
            return 0;
        } catch (Exception ex) {
            Log("ERROR: " + ex);
            return 1;
        }
    }
}
'@
Set-Content -LiteralPath $LauncherCs -Value $cs -Encoding ASCII
& $CscExe /nologo /target:winexe /optimize+ "/win32icon:$Icon" "/resource:$PayloadZip,Nukera.Payload" /r:System.IO.Compression.dll /r:System.IO.Compression.FileSystem.dll /r:System.Management.dll "/out:$OutExe" $LauncherCs
if ($LASTEXITCODE -ne 0) { throw "csc failed" }

Write-Host "==> DONE: $OutExe ($((Get-Item $OutExe).Length/1MB) MB)"