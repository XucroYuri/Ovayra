[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)][string]$SourceRoot,
  [Parameter(Mandatory = $true)][string]$DependencyPrefix,
  [Parameter(Mandatory = $true)][string]$StageRoot,
  [Parameter(Mandatory = $true)][ValidateRange(1, 256)][int]$Parallelism
)

$ErrorActionPreference = 'Stop'
$targetTriple = 'x86_64-pc-windows-msvc'
if ([string]::IsNullOrWhiteSpace($env:SOURCE_DATE_EPOCH)) { throw 'SOURCE_DATE_EPOCH must be set from FFmpeg n8.1.2' }
$marker = Join-Path $StageRoot '.ovayra-target'
if ((Test-Path -LiteralPath $StageRoot) -and (!(Test-Path -LiteralPath $marker) -or ((Get-Content -LiteralPath $marker -Raw).Trim() -ne $targetTriple))) { throw 'refusing cross-target stage overwrite' }
New-Item -ItemType Directory -Force -Path $StageRoot, (Join-Path $StageRoot 'provenance'), (Join-Path $StageRoot 'LICENSES'), (Join-Path $StageRoot 'sbom') | Out-Null
Set-Content -LiteralPath $marker -NoNewline -Value $targetTriple
$env:CFLAGS = "$env:CFLAGS /pathmap:$SourceRoot=/usr/src/ovayra"

Push-Location (Join-Path $SourceRoot 'libvpx')
try { .\configure --prefix="$DependencyPrefix" --disable-examples --disable-tools --enable-vp9-highbitdepth; & make "-j$Parallelism"; & make test; & make install } finally { Pop-Location }
Push-Location (Join-Path $SourceRoot 'opus')
try { .\configure --prefix="$DependencyPrefix" --disable-doc; & make "-j$Parallelism"; & make check; & make install } finally { Pop-Location }

$ffmpeg = Join-Path $SourceRoot 'ffmpeg'
$configure = @("--prefix=$StageRoot", '--disable-autodetect', '--disable-debug', '--disable-doc', '--disable-ffplay', '--disable-network', '--enable-ffmpeg', '--enable-ffprobe', '--enable-libopus', '--enable-libvpx', '--enable-version3', '--disable-gpl', '--disable-nonfree', '--enable-d3d11va', '--enable-mediafoundation', '--enable-nvenc', '--enable-nvdec', "--extra-cflags=-I$DependencyPrefix/include", "--extra-ldflags=-L$DependencyPrefix/lib")
Set-Content -LiteralPath (Join-Path $StageRoot 'provenance/buildconf.txt') -NoNewline -Value ('configuration: ' + ($configure -join ' '))
Push-Location $ffmpeg
try {
  $env:PKG_CONFIG_PATH = "$DependencyPrefix/lib/pkgconfig"
  & .\configure @configure; & make "-j$Parallelism"
  $fateTargets = (& make fate-list | Select-String -Pattern '^fate-(lavf-matroska|vp9|opus)' | Select-Object -First 3 | ForEach-Object { $_.Line })
  if ($fateTargets.Count -eq 0) { throw 'required FATE smoke targets unavailable' }
  & make @fateTargets; & make install
  & git diff --no-ext-diff | Set-Content -LiteralPath (Join-Path $StageRoot 'provenance/changes.diff') -NoNewline
} finally { Pop-Location }
Copy-Item -LiteralPath (Join-Path $SourceRoot 'ffmpeg-8.1.2.tar.xz'), (Join-Path $SourceRoot 'ffmpeg-8.1.2.tar.xz.asc'), (Join-Path $SourceRoot 'libvpx-source.tar.zst'), (Join-Path $SourceRoot 'opus-source.tar.zst') -Destination (Join-Path $StageRoot 'provenance')
Copy-Item -LiteralPath (Join-Path $SourceRoot 'ffmpeg/COPYING.LGPLv2.1') -Destination (Join-Path $StageRoot 'LICENSES/FFmpeg-LGPL-2.1-or-later.txt')
Copy-Item -LiteralPath (Join-Path $SourceRoot 'libvpx/LICENSE') -Destination (Join-Path $StageRoot 'LICENSES/libvpx-BSD-3-Clause.txt')
Copy-Item -LiteralPath (Join-Path $SourceRoot 'opus/COPYING') -Destination (Join-Path $StageRoot 'LICENSES/Opus-BSD-3-Clause.txt')
Copy-Item -LiteralPath (Join-Path $PSScriptRoot '../packaging/NOTICE.txt') -Destination (Join-Path $StageRoot 'NOTICE.txt')
Get-ChildItem -LiteralPath $StageRoot -File -Recurse | Where-Object { $_.Name -ne 'SHA256SUMS' } | Sort-Object FullName | ForEach-Object { $hash = Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256; "$($hash.Hash.ToLower())  $($_.FullName.Substring($StageRoot.Length + 1).Replace('\', '/'))" } | Set-Content -LiteralPath (Join-Path $StageRoot 'provenance/SHA256SUMS')
