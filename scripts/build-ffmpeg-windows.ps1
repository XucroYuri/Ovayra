[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)][string]$SourceRoot,
  [Parameter(Mandatory = $true)][string]$DependencyPrefix,
  [Parameter(Mandatory = $true)][string]$StageRoot,
  [Parameter(Mandatory = $true)][ValidateRange(1, 256)][int]$Parallelism
)

$ErrorActionPreference = 'Stop'
$expectedEpoch = '1781663615'
if ($env:SOURCE_DATE_EPOCH.Trim() -ne $expectedEpoch) { throw "SOURCE_DATE_EPOCH must equal locked value $expectedEpoch" }
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
if (!(Test-Path -LiteralPath $vswhere)) { throw "vswhere missing: $vswhere" }
$installation = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if ([string]::IsNullOrWhiteSpace($installation)) { throw 'Visual Studio MSVC x64 tools are missing' }
$vsDevCmd = Join-Path $installation 'Common7\Tools\VsDevCmd.bat'
if (!(Test-Path -LiteralPath $vsDevCmd)) { throw "VsDevCmd missing: $vsDevCmd" }
cmd.exe /d /s /c "`"$vsDevCmd`" -no_logo -arch=x64 -host_arch=x64 && set" | ForEach-Object {
  if ($_ -match '^([^=]+)=(.*)$') { Set-Item -Path "Env:$($Matches[1])" -Value $Matches[2] }
}
foreach ($tool in 'cl.exe', 'link.exe', 'lib.exe') {
  if (!(Get-Command $tool -ErrorAction SilentlyContinue)) { throw "MSVC tool unavailable after VsDevCmd: $tool" }
}
$env:OVAYRA_MSVC_BIN = Split-Path -Parent (Get-Command cl.exe).Source
foreach ($tool in 'cmake.exe', 'ninja.exe') {
  if (!(Get-Command $tool -ErrorAction SilentlyContinue)) { throw "native Windows build tool unavailable: $tool" }
}
$env:OVAYRA_NATIVE_CMAKE = (Get-Command cmake.exe).Source
$env:OVAYRA_NATIVE_NINJA = (Get-Command ninja.exe).Source
$msys2Location = $env:MSYS2_LOCATION
if ([string]::IsNullOrWhiteSpace($msys2Location)) { throw 'MSYS2_LOCATION is missing' }
$bash = Join-Path $msys2Location 'usr\bin\bash.exe'
if (!(Test-Path -LiteralPath $bash)) { throw "MSYS2 bash missing: $bash" }
$env:OVAYRA_MSYS_BIN = Split-Path -Parent $bash
foreach ($value in @($SourceRoot, $DependencyPrefix, $StageRoot)) {
  if ([string]::IsNullOrWhiteSpace($value) -or $value.Contains("`n") -or $value.Contains("`r")) { throw 'build paths must be nonempty and newline-free' }
}
$script = Join-Path $PSScriptRoot 'build-ffmpeg-windows-msys.sh'
$env:CC = 'cl'; $env:CXX = 'cl'; $env:AR = 'lib'; $env:LD = 'link'
& $bash --noprofile --norc $script --source-root $SourceRoot --dependency-prefix $DependencyPrefix --stage-root $StageRoot --parallelism $Parallelism
if ($LASTEXITCODE -ne 0) { throw "MSYS2/MSVC FFmpeg build failed with exit code $LASTEXITCODE" }
