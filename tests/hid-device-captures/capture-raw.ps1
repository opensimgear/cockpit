$ErrorActionPreference = 'Stop'
# Run on the host interactive desktop; register controller usages 4/5/8 only.
# No device handles, output/feature requests, input injection or vJoy operations.
New-Item -ItemType Directory -Path (Join-Path $PSScriptRoot '.generated') -Force | Out-Null
$source = Join-Path $PSScriptRoot '..\controller-mapping-m0\RawInputProbe.cs'
$binary = Join-Path $PSScriptRoot '.generated\RawInputProbe.exe'
& 'C:\Windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe' /nologo /platform:x64 /r:System.Web.Extensions.dll "/out:$binary" $source
if ($LASTEXITCODE -ne 0) { throw 'Probe compilation failed.' }
$started = [DateTime]::UtcNow.ToString('o')
$capture = & $binary --physical-corpus
if ($LASTEXITCODE -ne 0) { throw 'Probe capture failed.' }
$record = $capture | ConvertFrom-Json
$record | Add-Member -NotePropertyName started -NotePropertyValue $started
$record | Add-Member -NotePropertyName ended -NotePropertyValue ([DateTime]::UtcNow.ToString('o'))
$record | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $PSScriptRoot '.generated\raw-input.json') -Encoding utf8
