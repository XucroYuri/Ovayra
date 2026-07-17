[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)][ValidateSet('msi')][string]$Kind,
  [Parameter(Mandatory = $true)][string]$Artifact,
  [switch]$RequireNv
)

$ErrorActionPreference = 'Stop'

function Require-RegularFile([string]$Root, [string]$Relative) {
  $path = Join-Path $Root $Relative
  $item = Get-Item -LiteralPath $path -Force -ErrorAction SilentlyContinue
  if ($null -eq $item -or $item.PSIsContainer -or $null -ne $item.LinkType -or $item.Length -le 0) {
    throw "missing, empty, or non-regular required resource: $Relative"
  }
}

function Find-One([string]$Root, [string]$LeafName) {
  $items = @(Get-ChildItem -LiteralPath $Root -Recurse -Force -File | Where-Object { $_.Name -eq $LeafName })
  if ($items.Count -ne 1) { throw "expected exactly one $LeafName under $Root" }
  if ($null -ne $items[0].LinkType) { throw "required file is a symlink: $($items[0].FullName)" }
  return $items[0]
}

function Inspect-ExtractedTree([string]$Root, [bool]$NeedsNv) {
  $notice = @(Get-ChildItem -LiteralPath $Root -Recurse -Force -File | Where-Object {
    $_.Name -eq 'NOTICE.txt' -and $_.FullName -match '[\\/]ffmpeg[\\/]NOTICE\.txt$'
  })
  if ($notice.Count -ne 1) { throw "expected exactly one ffmpeg/NOTICE.txt under $Root" }
  if ($null -ne $notice[0].LinkType) { throw "ffmpeg NOTICE is a symlink: $($notice[0].FullName)" }
  $resourceRoot = Split-Path -Parent (Split-Path -Parent $notice[0].FullName)
  $app = Find-One $Root 'ovayra-spike.exe'
  if ($app.Length -le 0) { throw 'application executable is empty' }
  $required = @(
    'NOTICE.txt', 'ffmpeg/NOTICE.txt', 'ffmpeg/bin/ffmpeg.exe', 'ffmpeg/bin/ffprobe.exe',
    'ffmpeg/LICENSES/FFmpeg-LGPL-2.1-or-later.txt', 'ffmpeg/LICENSES/libvpx-BSD-3-Clause.txt',
    'ffmpeg/LICENSES/Opus-BSD-3-Clause.txt', 'ffmpeg/provenance/ffmpeg.lock',
    'ffmpeg/provenance/ffmpeg-8.1.2.tar.xz', 'ffmpeg/provenance/ffmpeg-8.1.2.tar.xz.asc',
    'ffmpeg/provenance/ffmpeg-signature-attestation.json', 'ffmpeg/provenance/libvpx-source.tar.zst',
    'ffmpeg/provenance/opus-source.tar.zst', 'ffmpeg/provenance/buildconf.txt',
    'ffmpeg/provenance/changes.diff', 'ffmpeg/provenance/SHA256SUMS', 'ffmpeg/sbom/ffmpeg.cdx.json'
  )
  if ($NeedsNv) { $required += 'ffmpeg/LICENSES/nv-codec-headers-MIT.txt', 'ffmpeg/provenance/nv-codec-headers-source.tar.zst' }
  foreach ($relative in $required) { Require-RegularFile $resourceRoot $relative }
  $tuples = @(Get-ChildItem -LiteralPath $Root -Recurse -Force -File | Sort-Object FullName | ForEach-Object {
    $relative = $_.FullName.Substring($Root.Length).TrimStart('\', '/')
    "$relative`t$((Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant())`t$($_.Length)"
  })
  $bytes = [Text.Encoding]::UTF8.GetBytes(($tuples -join "`n") + "`n")
  $hash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($bytes)).ToLowerInvariant()
  Write-Output "INSPECTION_TREE_SHA256=$hash"
}

if (!(Test-Path -LiteralPath $Artifact -PathType Leaf)) { throw "missing MSI: $Artifact" }
$destination = Join-Path $env:RUNNER_TEMP ("phase-0-msi-inspect-" + [guid]::NewGuid().ToString('N'))
$log = Join-Path $env:RUNNER_TEMP ("phase-0-msi-inspect-" + [guid]::NewGuid().ToString('N') + '.log')
New-Item -ItemType Directory -Force $destination | Out-Null
try {
  $process = Start-Process -FilePath 'msiexec.exe' -Wait -PassThru -ArgumentList @('/a', $Artifact, "TARGETDIR=$destination", '/qn', '/l*v', $log)
  if ($process.ExitCode -ne 0) { throw "MSI administrative extraction failed with exit code $($process.ExitCode); log: $log" }
  if (!(Test-Path -LiteralPath $log -PathType Leaf)) { throw 'MSI extraction log was not created' }
  Inspect-ExtractedTree $destination $RequireNv.IsPresent
} finally {
  Remove-Item -LiteralPath $destination -Recurse -Force -ErrorAction SilentlyContinue
}
