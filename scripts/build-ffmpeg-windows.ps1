[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)][string]$SourceRoot,
  [Parameter(Mandatory = $true)][string]$DependencyPrefix,
  [Parameter(Mandatory = $true)][string]$StageRoot,
  [Parameter(Mandatory = $true)][ValidateRange(1, 256)][int]$Parallelism
)

$ErrorActionPreference = 'Stop'
if ([string]::IsNullOrWhiteSpace($env:SOURCE_DATE_EPOCH)) { throw 'SOURCE_DATE_EPOCH must be set from FFmpeg n8.1.2' }
$vsDevCmd = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\2022\Enterprise\Common7\Tools\VsDevCmd.bat'
if (!(Test-Path -LiteralPath $vsDevCmd)) { throw "MSVC developer environment missing: $vsDevCmd" }
cmd.exe /c "`"$vsDevCmd`" -arch=x64 -host_arch=x64 && set" | ForEach-Object {
  if ($_ -match '^([^=]+)=(.*)$') { Set-Item -Path "Env:$($Matches[1])" -Value $Matches[2] }
}
$bash = 'C:\msys64\usr\bin\bash.exe'
if (!(Test-Path -LiteralPath $bash)) { throw "MSYS2 bash missing: $bash" }
foreach ($value in @($SourceRoot, $DependencyPrefix, $StageRoot)) { if ($value.Contains("'")) { throw 'single quote in build path is unsupported' } }
$script = Join-Path $PSScriptRoot 'build-ffmpeg-windows-msys.sh'
$command = "'$(($script -replace '\\', '/'))' --source-root '$(($SourceRoot -replace '\\', '/'))' --dependency-prefix '$(($DependencyPrefix -replace '\\', '/'))' --stage-root '$(($StageRoot -replace '\\', '/'))' --parallelism '$Parallelism'"
& $bash -lc $command
if ($LASTEXITCODE -ne 0) { throw "MSYS2 FFmpeg build failed with exit code $LASTEXITCODE" }
