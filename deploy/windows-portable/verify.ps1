[CmdletBinding()]
param([string]$Root=$PSScriptRoot)
$ErrorActionPreference='Stop'
$m=Get-Content -LiteralPath (Join-Path $Root 'release-manifest.json') -Raw | ConvertFrom-Json
if ($m.schema_version -ne 1 -or @($m.files).Count -eq 0) { throw 'Invalid/empty release manifest' }
$seen=@{}
foreach ($f in $m.files) {
    if ($f.path -match '(^[\\/]|:|(^|[\\/])\.\.([\\/]|$))') { throw 'Unsafe manifest path' }
    $p=Join-Path $Root $f.path
    $key=$f.path.Replace('\','/').ToLowerInvariant()
    if ($key -match '(^/|:|//|(^|/)\.{1,2}(/|$)|[. ](/|$))') { throw 'Unsafe normalized manifest path' }
    if ($seen.ContainsKey($key)) { throw 'Duplicate normalized manifest path' }; $seen[$key]=$true
    if ($f.sha256 -notmatch '^[a-fA-F0-9]{64}$') { throw 'Invalid SHA256' }
    $walk=[IO.Path]::GetFullPath($p)
    while ($walk -and $walk.Length -ge ([IO.Path]::GetFullPath($Root)).Length) {
        if ((Get-Item -LiteralPath $walk -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Reparse payload paths are forbidden' }
        $walk=Split-Path $walk -Parent
    }
    if (!(Test-Path -LiteralPath $p -PathType Leaf) -or (Get-FileHash -LiteralPath $p -Algorithm SHA256).Hash -ne $f.sha256) { throw "Integrity failure: $($f.path)" }
}
foreach ($required in @('ocr-pipeline.exe','pdfium.dll','tessdata/eng.traineddata','tessdata/rus.traineddata')) { if (!$seen.ContainsKey($required)) { throw "Required manifest file missing: $required" } }
$expected=@($seen.Keys)+@('release-manifest.json')
foreach ($f in Get-ChildItem -LiteralPath $Root -Recurse -File) {
    $relative=$f.FullName.Substring(([IO.Path]::GetFullPath($Root)).TrimEnd('\').Length+1).Replace('\','/')
    if ($relative -notin $expected) { throw "Unexpected package file: $relative" }
}
Write-Output 'Payload hashes verified. This is not a signature or licensing approval.'
