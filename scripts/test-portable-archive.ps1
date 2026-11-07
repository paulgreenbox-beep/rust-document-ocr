# Synthetic transactional ZIP tests. No Cargo, OCR executable, VM or network.
[CmdletBinding()]
param([string]$WorkRoot=[IO.Path]::GetTempPath())
$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot 'portable-archive.ps1')

$work=Join-Path $WorkRoot ('ocr-portable-archive-tests-'+[guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $work | Out-Null
$originalWriter=${function:Write-PortablePendingZip}

function Invoke-SyntheticArchiveCase {
    param([string]$Label,[string]$Fault,[string]$ExpectedError,[scriptblock]$RewriteZip)
    $caseRoot=Join-Path $work $Label
    $package=Join-Path $caseRoot 'Portable OCR package'
    $final="$package.zip"
    New-Item -ItemType Directory -Path (Join-Path $package 'tessdata'),(Join-Path $package 'empty') | Out-Null
    $files=@(foreach ($name in @('payload.txt','tessdata/synthetic.txt','[literal].txt')) {
        $path=Join-Path $package $name
        [IO.File]::WriteAllText($path,'synthetic '+$name)
        @{path=$name;size=(Get-Item -LiteralPath $path).Length;sha256=(Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash}
    })
    $manifestPath=Join-Path $package 'release-manifest.json'
    @{schema_version=1;files=$files} | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $manifestPath -Encoding utf8
    $manifestHash=(Get-FileHash -LiteralPath $manifestPath -Algorithm SHA256).Hash
    if ($Fault -eq 'manifest-drift') { [IO.File]::AppendAllText($manifestPath,"`n ") }
    if ($Fault -eq 'hidden') {
        $hidden=Join-Path $package 'unlisted.txt'
        [IO.File]::WriteAllText($hidden,'synthetic unlisted hidden file')
        [IO.File]::SetAttributes($hidden,[IO.FileAttributes]::Hidden)
    }
    if ($Fault -eq 'preexisting') { [IO.File]::WriteAllText($final,'previous ZIP bytes') }

    # Scope-local interception of private I/O operations; production exposes no fault switches.
    if ($Fault -eq 'compression' -or $RewriteZip) {
        function Write-PortablePendingZip($Snapshot,[string]$PendingPath) {
            if ($Fault -eq 'compression') {
                [IO.File]::WriteAllText($PendingPath,'partial compression evidence')
                throw 'synthetic compression failure'
            }
            & $originalWriter $Snapshot $PendingPath
            $file=[IO.File]::Open($PendingPath,[IO.FileMode]::Open,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None)
            try {
                $zip=[IO.Compression.ZipArchive]::new($file,[IO.Compression.ZipArchiveMode]::Update,$true,[Text.Encoding]::UTF8)
                try { & $RewriteZip $zip $Snapshot } finally { $zip.Dispose() }
            } finally { $file.Dispose() }
        }
    }
    if ($Fault -eq 'extraction') {
        function Expand-PortablePendingZip($Zip,[string]$ExtractionPath) {
            New-Item -ItemType Directory -Path $ExtractionPath | Out-Null
            [IO.File]::WriteAllText((Join-Path $ExtractionPath 'partial.txt'),'partial extraction evidence')
            throw 'synthetic extraction failure'
        }
    }
    $state=@{smoke=$false;baseline=$false;pendingHash=$null;lease=$null}
    $failure=$null; $result=@()
    try {
        $result=@(Publish-PortableArchive -PackageRoot $package -FinalPath $final -ExpectedManifestSha256 $manifestHash -Smoke {
            param($archiveRoot)
            if (Test-Path -LiteralPath $final) { throw 'Test observed final ZIP before checks completed' }
            if (!(Test-Path -LiteralPath $package -PathType Container) -or !(Test-Path -LiteralPath $archiveRoot -PathType Container) -or $archiveRoot -eq $package) {
                throw 'Test expected separate extracted and staged trees'
            }
            $pending=@(Get-ChildItem -LiteralPath $caseRoot -Force -File -Filter '*.zip.pending')
            if ($pending.Count -ne 1) { throw 'Test expected exactly one non-release pending archive during smoke' }
            $state.pendingHash=(Get-FileHash -LiteralPath $pending[0].FullName -Algorithm SHA256).Hash
            $writeBlocked=$false
            try {
                $writer=[IO.File]::Open($pending[0].FullName,[IO.FileMode]::Open,[IO.FileAccess]::Write,[IO.FileShare]::ReadWrite)
                $writer.Dispose()
            } catch [IO.IOException] { $writeBlocked=$true }
            if (!$writeBlocked) { throw 'Test was able to open the pending archive for write during smoke' }
            $state.smoke=$true
            if ($Fault -eq 'smoke') { throw 'synthetic extracted smoke failure' }
            $state.baseline=$true
            if (Test-Path -LiteralPath $final) { throw 'Test observed final ZIP during baseline' }
            if ($Fault -eq 'baseline') { throw 'synthetic baseline failure' }
            if ($Fault -eq 'smoke-mutation') { [IO.File]::AppendAllText((Join-Path $archiveRoot 'payload.txt'),'changed by smoke') }
            if ($Fault -eq 'move') {
                # Windows denies the real atomic rename while this second reader denies delete-sharing.
                $state.lease=[IO.File]::Open($pending[0].FullName,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read)
            }
            if ($Fault -eq 'late-collision') { [IO.File]::WriteAllText($final,'previous ZIP bytes') }
            Write-Output 'Synthetic smoke output (not a publication receipt)'
        })
    } catch { $failure=$_.Exception.Message }
    finally { if ($null -ne $state.lease) { $state.lease.Dispose() } }

    $pendingCount=@(Get-ChildItem -LiteralPath $caseRoot -Force -File -Filter '*.zip.pending').Count
    $extractCount=@(Get-ChildItem -LiteralPath $caseRoot -Force -Directory -Filter 'zip-smoke-*').Count
    if (!(Test-Path -LiteralPath $package -PathType Container)) { throw "$Label lost the staged package" }
    foreach ($entry in $files) {
        if ((Get-FileHash -LiteralPath (Join-Path $package $entry.path) -Algorithm SHA256).Hash -cne $entry.sha256) { throw "$Label changed staged payload bytes" }
    }
    if (!$ExpectedError) {
        if ($failure) { throw "Unexpected $Label failure: $failure" }
        if ($result.Count -ne 1 -or $result[0].Algorithm -cne 'SHA256' -or $result[0].Path -cne $final -or
            $result[0].Hash -cne $state.pendingHash -or (Get-FileHash -LiteralPath $final -Algorithm SHA256).Hash -cne $result[0].Hash -or
            !$state.smoke -or !$state.baseline -or $pendingCount -ne 0 -or $extractCount -ne 1) {
            throw 'Success did not publish the exact validated bytes with matching checksum and retained extraction'
        }
    } else {
        if (!$failure -or $failure -notmatch $ExpectedError) { throw "$Label expected '$ExpectedError' rejection, got: $failure" }
        if ($result.Count) { throw "$Label returned a publication receipt on failure" }
        if ($Fault -in @('preexisting','late-collision')) {
            if ([IO.File]::ReadAllText($final) -cne 'previous ZIP bytes') { throw "$Label overwrote an existing final ZIP" }
        } elseif (Test-Path -LiteralPath $final) { throw "$Label exposed a final ZIP on failure" }
        $noStage=$Fault -in @('preexisting','manifest-drift','hidden')
        if ($pendingCount -ne [int](!$noStage)) { throw "$Label wrong pending archive evidence count: $pendingCount" }
        $shouldExtract=$Fault -in @('extraction','integrity','smoke','baseline','smoke-mutation','move','late-collision')
        if ($extractCount -ne [int]$shouldExtract) { throw "$Label wrong extraction evidence count: $extractCount" }
        if ($state.smoke -ne ($Fault -in @('smoke','baseline','smoke-mutation','move','late-collision')) -or
            $state.baseline -ne ($Fault -in @('baseline','smoke-mutation','move','late-collision'))) { throw "$Label ran the wrong validation stages" }
    }
    Write-Output "PASS: $Label"
}

try {
    Invoke-SyntheticArchiveCase 'compression-failure' 'compression' 'synthetic compression failure'
    Invoke-SyntheticArchiveCase 'extraction-failure' 'extraction' 'synthetic extraction failure'
    Invoke-SyntheticArchiveCase 'same-size-wrong-content' 'integrity' 'Extracted ZIP integrity failure' {
        param($zip,$snapshot)
        $entry=$zip.GetEntry($snapshot.Leaf+'/payload.txt')
        $stream=$entry.Open()
        try { $stream.WriteByte([byte][char]'X') } finally { $stream.Dispose() }
    }
    Invoke-SyntheticArchiveCase 'manifest-bytes-changed-in-zip' 'integrity' 'Extracted ZIP integrity failure: release-manifest.json' {
        param($zip,$snapshot)
        $entry=$zip.GetEntry($snapshot.Leaf+'/release-manifest.json')
        $stream=$entry.Open()
        try { $stream.Position=$stream.Length-1; $stream.WriteByte([byte][char]' ') } finally { $stream.Dispose() }
    }
    Invoke-SyntheticArchiveCase 'traversal-before-extraction' 'entry' 'Unsafe ZIP path' {
        param($zip,$snapshot)
        $zip.CreateEntry($snapshot.Leaf+'/../outside.txt') | Out-Null
    }
    Invoke-SyntheticArchiveCase 'rooted-before-extraction' 'entry' 'Unsafe ZIP path' {
        param($zip,$snapshot)
        $zip.CreateEntry('/rooted.txt') | Out-Null
    }
    Invoke-SyntheticArchiveCase 'alternate-stream-before-extraction' 'entry' 'Unsafe ZIP path' {
        param($zip,$snapshot)
        $zip.CreateEntry($snapshot.Leaf+'/payload.txt:stream') | Out-Null
    }
    Invoke-SyntheticArchiveCase 'backslash-alias-before-extraction' 'entry' 'Unsafe ZIP path' {
        param($zip,$snapshot)
        $zip.CreateEntry($snapshot.Leaf+'\payload.txt') | Out-Null
    }
    Invoke-SyntheticArchiveCase 'reserved-before-extraction' 'entry' 'Unsafe ZIP path' {
        param($zip,$snapshot)
        $zip.CreateEntry($snapshot.Leaf+'/NUL.txt') | Out-Null
    }
    Invoke-SyntheticArchiveCase 'case-duplicate-before-extraction' 'entry' 'Duplicate or ambiguous ZIP entry' {
        param($zip,$snapshot)
        $zip.CreateEntry($snapshot.Leaf+'/PAYLOAD.TXT').ExternalAttributes=0
    }
    Invoke-SyntheticArchiveCase 'reparse-before-extraction' 'entry' 'Unexpected ZIP file attributes' {
        param($zip,$snapshot)
        $zip.GetEntry($snapshot.Leaf+'/payload.txt').ExternalAttributes=[int][IO.FileAttributes]::ReparsePoint
    }
    Invoke-SyntheticArchiveCase 'hidden-before-extraction' 'entry' 'Unexpected ZIP file attributes' {
        param($zip,$snapshot)
        $zip.GetEntry($snapshot.Leaf+'/payload.txt').ExternalAttributes=[int][IO.FileAttributes]::Hidden
    }
    Invoke-SyntheticArchiveCase 'wrong-size-before-extraction' 'entry' 'Unexpected ZIP file name or size' {
        param($zip,$snapshot)
        $entry=$zip.GetEntry($snapshot.Leaf+'/payload.txt')
        $stream=$entry.Open()
        try { $stream.SetLength($stream.Length+1) } finally { $stream.Dispose() }
    }
    Invoke-SyntheticArchiveCase 'unlisted-before-extraction' 'entry' 'Unexpected ZIP file name or size' {
        param($zip,$snapshot)
        $zip.CreateEntry($snapshot.Leaf+'/extra.txt').ExternalAttributes=0
    }
    Invoke-SyntheticArchiveCase 'missing-manifest-before-extraction' 'entry' 'ZIP entries are missing' {
        param($zip,$snapshot)
        $zip.GetEntry($snapshot.Leaf+'/release-manifest.json').Delete()
    }
    Invoke-SyntheticArchiveCase 'hidden-staged-file' 'hidden' 'hidden, system or device'
    Invoke-SyntheticArchiveCase 'staged-manifest-drift' 'manifest-drift' 'manifest changed'
    Invoke-SyntheticArchiveCase 'smoke-failure' 'smoke' 'synthetic extracted smoke failure'
    Invoke-SyntheticArchiveCase 'baseline-failure' 'baseline' 'synthetic baseline failure'
    Invoke-SyntheticArchiveCase 'smoke-mutation' 'smoke-mutation' 'Extracted ZIP integrity failure'
    Invoke-SyntheticArchiveCase 'move-failure' 'move' 'Portable ZIP publication failed:.*Move'
    Invoke-SyntheticArchiveCase 'final-preexists' 'preexisting' 'Final ZIP exists; refusing overwrite'
    Invoke-SyntheticArchiveCase 'final-created-before-move' 'late-collision' 'Portable ZIP publication failed:.*Move'
    Invoke-SyntheticArchiveCase 'publish-success'
    Write-Output 'Transactional ZIP source-only checks PASS (synthetic archive, not native OCR acceptance)'
} finally {
    Write-Output "Retained synthetic archive test evidence: $work"
}
