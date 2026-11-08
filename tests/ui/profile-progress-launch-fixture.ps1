param([Parameter(Mandatory)][string]$RunDirectory)
$ErrorActionPreference = 'Stop'
$run = (Resolve-Path -LiteralPath $RunDirectory).Path
$programs = Join-Path $run 'programs'
$steam = Join-Path $programs 'Steam'
$runtime = Join-Path $steam 'steamapps/common/ProgressFixture'
if (Test-Path -LiteralPath $programs) { throw 'Use a new disposable run directory' }
New-Item -ItemType Directory -Path $runtime | Out-Null
$source = Join-Path $run 'ProgressLauncher.cs'
[IO.File]::WriteAllText($source, @'
using System;
using System.IO;

internal static class ProgressLauncher
{
    private static void Main(string[] args)
    {
        File.WriteAllText(Path.Combine(AppDomain.CurrentDomain.BaseDirectory, "launch.started"), String.Join(" ", args));
    }
}
'@)
$compiler = Join-Path $env:WINDIR 'Microsoft.NET/Framework64/v4.0.30319/csc.exe'
$launcher = Join-Path $steam 'steam.exe'
& $compiler /nologo /target:winexe "/out:$launcher" $source
if ($LASTEXITCODE) { exit $LASTEXITCODE }
Copy-Item -LiteralPath $launcher -Destination (Join-Path $runtime 'ProgressRuntime.exe')
[IO.File]::WriteAllText((Join-Path $steam 'steamapps/appmanifest_426196196.acf'), @'
"AppState"
{
  "appid" "426196196"
  "name" "Progress fixture"
  "installdir" "ProgressFixture"
}
'@)
