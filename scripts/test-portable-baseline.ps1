param(
    [Parameter(Mandatory=$true)]
    [ValidateNotNullOrEmpty()]
    [string]$WorkRoot
)

$ErrorActionPreference = 'Stop'

$helper = Join-Path $PSScriptRoot 'portable-baseline.ps1'
if (!(Test-Path -LiteralPath $helper -PathType Leaf)) {
    throw "Portable baseline helper is missing: $helper"
}
. $helper

if (!(Get-Command Invoke-PortableBaselineComparison -CommandType Function -ErrorAction SilentlyContinue)) {
    throw 'portable-baseline.ps1 did not define Invoke-PortableBaselineComparison.'
}

$resolvedWorkRoot = [IO.Path]::GetFullPath($WorkRoot)
if (!(Test-Path -LiteralPath $resolvedWorkRoot -PathType Container)) {
    New-Item -ItemType Directory -Path $resolvedWorkRoot | Out-Null
}
$runRoot = Join-Path $resolvedWorkRoot ('portable-baseline-tests-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $runRoot | Out-Null

$relativeFiles = @(
    'text-table/result.json',
    'text-table/result.v1.json',
    'text-table/result.docx',
    'text-table/result.xlsx',
    'raster/result.json',
    'raster/result.v1.json',
    'raster/result.docx',
    'raster/result.xlsx'
)

function Get-CanonicalPath([string]$Path) {
    return [IO.Path]::GetFullPath($Path).TrimEnd([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar)
}

function Write-ManifestFile([string]$Path, $Manifest) {
    $json = ConvertTo-Json -InputObject $Manifest -Depth 10
    [IO.File]::WriteAllText($Path, $json)
}

function New-SyntheticTree {
    param(
        [Parameter(Mandatory=$true)][string]$CaseName,
        [switch]$JunctionTextTable,
        [switch]$LegacyManifest
    )

    $caseRoot = Join-Path $runRoot $CaseName
    New-Item -ItemType Directory -Path $caseRoot | Out-Null
    $baselineRoot = Join-Path $caseRoot 'baseline'
    $snapshotRoot = Join-Path $caseRoot 'snapshot'
    New-Item -ItemType Directory -Path $baselineRoot | Out-Null

    $outsideTextTable = Join-Path $caseRoot 'outside-text-table'
    if ($JunctionTextTable) {
        New-Item -ItemType Directory -Path $outsideTextTable | Out-Null
        New-Item -ItemType Directory -Path (Join-Path $baselineRoot 'raster') | Out-Null
    } else {
        New-Item -ItemType Directory -Path (Join-Path $baselineRoot 'text-table') | Out-Null
        New-Item -ItemType Directory -Path (Join-Path $baselineRoot 'raster') | Out-Null
    }
    foreach ($relative in $relativeFiles) {
        $baselineDestination = Join-Path $baselineRoot ($relative -replace '/', '\')
        if ($JunctionTextTable -and $relative.StartsWith('text-table/')) {
            $baselineDestination = Join-Path $outsideTextTable ($relative.Substring('text-table/'.Length) -replace '/', '\')
        }
        $baselineParent = Split-Path -Parent $baselineDestination
        if (!(Test-Path -LiteralPath $baselineParent -PathType Container)) {
            New-Item -ItemType Directory -Path $baselineParent | Out-Null
        }

        # These are deliberately synthetic text bytes, including for .docx/.xlsx.
        $label = [IO.Path]::GetFileName($relative)
        $contents = "{{`"fixture`":`"$CaseName/$relative`",`"name`":`"$label`"}}`n"
        [IO.File]::WriteAllText($baselineDestination, $contents)
    }

    if ($JunctionTextTable) {
        $junction = Join-Path $baselineRoot 'text-table'
        New-Item -ItemType Junction -Path $junction -Target $outsideTextTable | Out-Null
        $junctionItem = Get-Item -LiteralPath $junction -Force
        if (($junctionItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -eq 0) {
            throw 'Junction fixture was not created as a real reparse point.'
        }
    }

    $entries = @()
    foreach ($relative in $relativeFiles) {
        if ($JunctionTextTable -and $relative.StartsWith('text-table/')) {
            $filePath = Join-Path $outsideTextTable ($relative.Substring('text-table/'.Length) -replace '/', '\')
        } else {
            $filePath = Join-Path $baselineRoot ($relative -replace '/', '\')
        }
        $file = Get-Item -LiteralPath $filePath
        $hash = (Get-FileHash -LiteralPath $filePath -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($LegacyManifest) {
            $entries += [pscustomobject]@{ path=$relative; sha256=$hash }
        } else {
            $entries += [pscustomobject]@{ path=$relative; size=[long]$file.Length; sha256=$hash }
        }
    }

    if ($LegacyManifest) {
        $manifestObject = [pscustomobject]@{
            scope = 'text-table|raster'
            exe_sha256 = ('a' * 64)
            runtime_manifest_sha256 = ('b' * 64)
            files = $entries
        }
    } else {
        $manifestObject = [pscustomobject]@{ schema_version=1; files=$entries }
    }
    $manifestPath = Join-Path $caseRoot 'baseline-manifest.json'
    Write-ManifestFile -Path $manifestPath -Manifest $manifestObject

    return [pscustomobject]@{
        CaseRoot = $caseRoot
        BaselineRoot = $baselineRoot
        SnapshotRoot = $snapshotRoot
        ManifestPath = $manifestPath
    }
}

function New-CompareProbe {
    $state = [pscustomobject]@{ Started=$false; VerifiedRoot=$null }
    $callback = {
        param($verifiedRoot)
        $state.Started = $true
        $state.VerifiedRoot = [string]$verifiedRoot
    }.GetNewClosure()
    return [pscustomobject]@{ State=$state; Callback=$callback }
}

function Invoke-ExpectedRejection {
    param(
        [Parameter(Mandatory=$true)]$Fixture,
        [Parameter(Mandatory=$true)][string]$Label,
        [string]$ExpectedManifestSha256,
        [string]$ExpectedDiagnostic,
        [switch]$RequireVersionedManifest
    )
    $probe = New-CompareProbe
    $rejected = $false
    try {
        $parameters = @{
            ManifestPath = $Fixture.ManifestPath
            BaselineRoot = $Fixture.BaselineRoot
            SnapshotRoot = $Fixture.SnapshotRoot
            Compare = $probe.Callback
        }
        if ($ExpectedManifestSha256) { $parameters.ExpectedManifestSha256 = $ExpectedManifestSha256 }
        if ($RequireVersionedManifest) { $parameters.RequireVersionedManifest = $true }
        Invoke-PortableBaselineComparison @parameters | Out-Null
    } catch {
        $rejected = $true
        if ($ExpectedDiagnostic -and $_.Exception.Message -notlike "*$ExpectedDiagnostic*") {
            throw "Wrong rejection for $Label; expected diagnostic containing '$ExpectedDiagnostic', got '$($_.Exception.Message)'"
        }
    }
    if (!$rejected) { throw "Expected rejection: $Label" }
    if ($probe.State.Started) { throw "Comparison callback ran before rejecting: $Label" }
    Write-Output "PASS: $Label (rejected before comparison)"
}

function Invoke-ExpectedComparison {
    param(
        [Parameter(Mandatory=$true)]$Fixture,
        [Parameter(Mandatory=$true)][string]$Label,
        [string]$ExpectedManifestSha256,
        [switch]$RequireVersionedManifest
    )
    $probe = New-CompareProbe
    $parameters = @{
        ManifestPath = $Fixture.ManifestPath
        BaselineRoot = $Fixture.BaselineRoot
        SnapshotRoot = $Fixture.SnapshotRoot
        Compare = $probe.Callback
    }
    if ($ExpectedManifestSha256) { $parameters.ExpectedManifestSha256 = $ExpectedManifestSha256 }
    if ($RequireVersionedManifest) { $parameters.RequireVersionedManifest = $true }
    Invoke-PortableBaselineComparison @parameters | Out-Null
    if (!$probe.State.Started) { throw "Comparison callback was not invoked: $Label" }
    if ((Get-CanonicalPath $probe.State.VerifiedRoot) -ne (Get-CanonicalPath $Fixture.SnapshotRoot)) {
        throw "Comparison callback received an unexpected verified snapshot root: $Label"
    }
    Write-Output "PASS: $Label (comparison received isolated verified snapshot)"
}

function Invoke-PositiveIsolationComparison {
    param(
        [Parameter(Mandatory=$true)]$Fixture,
        [Parameter(Mandatory=$true)][string]$Label,
        [string]$ExpectedManifestSha256
    )
    $manifestObject = Get-Content -LiteralPath $Fixture.ManifestPath -Raw | ConvertFrom-Json
    $expectedHash = @($manifestObject.files | Where-Object { $_.path -eq 'text-table/result.json' })[0].sha256
    $sourcePath = Join-Path $Fixture.BaselineRoot 'text-table\result.json'
    $snapshotPath = Join-Path $Fixture.SnapshotRoot 'text-table\result.json'
    $state = [pscustomobject]@{ Started=$false; VerifiedRoot=$null; SnapshotHash=$null; SourceHash=$null; SnapshotWriteBlocked=$false }
    $callback = {
        param($verifiedRoot)
        $state.Started = $true
        $state.VerifiedRoot = [string]$verifiedRoot
        $bytes = [IO.File]::ReadAllBytes($sourcePath)
        $bytes[0] = [byte]($bytes[0] -bxor 1)
        [IO.File]::WriteAllBytes($sourcePath, $bytes)
        try { [IO.File]::WriteAllBytes($snapshotPath, $bytes) }
        catch [IO.IOException] { $state.SnapshotWriteBlocked=$true }
        $state.SnapshotHash = (Get-FileHash -LiteralPath $snapshotPath -Algorithm SHA256).Hash.ToLowerInvariant()
        $state.SourceHash = (Get-FileHash -LiteralPath $sourcePath -Algorithm SHA256).Hash.ToLowerInvariant()
    }.GetNewClosure()
    $parameters = @{
        ManifestPath = $Fixture.ManifestPath
        BaselineRoot = $Fixture.BaselineRoot
        SnapshotRoot = $Fixture.SnapshotRoot
        Compare = $callback
    }
    if ($ExpectedManifestSha256) { $parameters.ExpectedManifestSha256 = $ExpectedManifestSha256 }
    Invoke-PortableBaselineComparison @parameters | Out-Null
    if (!$state.Started) { throw "Comparison callback was not invoked: $Label" }
    if ((Get-CanonicalPath $state.VerifiedRoot) -ne (Get-CanonicalPath $Fixture.SnapshotRoot)) {
        throw "Comparison callback received an unexpected verified snapshot root: $Label"
    }
    if (!$state.SnapshotWriteBlocked -or $state.SnapshotHash -ne $expectedHash -or $state.SourceHash -eq $expectedHash) {
        throw 'Snapshot bytes changed with the source, or the source mutation was ineffective.'
    }
    Write-Output "PASS: $Label (snapshot remains isolated from later source mutation)"
}

function Set-SameSizeMutation([string]$Path) {
    $bytes = [IO.File]::ReadAllBytes($Path)
    if ($bytes.Length -lt 1) { throw "Synthetic fixture unexpectedly empty: $Path" }
    $bytes[0] = [byte]($bytes[0] -bxor 1)
    [IO.File]::WriteAllBytes($Path, $bytes)
}

# A valid v1 manifest is accepted, and only its verified baseline is handed to Compare.
$valid = New-SyntheticTree -CaseName 'valid-v1'
$manifestHash = (Get-FileHash -LiteralPath $valid.ManifestPath -Algorithm SHA256).Hash.ToLowerInvariant()
Invoke-ExpectedComparison -Fixture $valid -Label 'valid v1 baseline' -ExpectedManifestSha256 $manifestHash -RequireVersionedManifest
$isolation = New-SyntheticTree -CaseName 'verified-snapshot-isolation'
$isolationHash = (Get-FileHash -LiteralPath $isolation.ManifestPath -Algorithm SHA256).Hash.ToLowerInvariant()
Invoke-PositiveIsolationComparison -Fixture $isolation -Label 'verified snapshot is isolated' -ExpectedManifestSha256 $isolationHash

# Legacy local manifests remain readable unless the caller explicitly requires v1.
$legacy = New-SyntheticTree -CaseName 'legacy-local' -LegacyManifest
Invoke-ExpectedComparison -Fixture $legacy -Label 'legacy local non-ZIP baseline'
Invoke-ExpectedRejection -Fixture $legacy -Label 'legacy rejected when versioned manifest is required' -ExpectedDiagnostic 'ZIP requires baseline manifest schema_version 1 with sizes' -RequireVersionedManifest

# Equal-size edits to JSON, DOCX, and XLSX artifacts must not be mistaken for an unchanged baseline.
foreach ($relative in @('text-table/result.json','raster/result.v1.json','text-table/result.docx','raster/result.xlsx')) {
    $caseName = 'changed-' + ([IO.Path]::GetFileName($relative) -replace '\.', '-') + '-' + ([guid]::NewGuid().ToString('N').Substring(0,8))
    $fixture = New-SyntheticTree -CaseName $caseName
    $path = Join-Path $fixture.BaselineRoot ($relative -replace '/', '\')
    $beforeLength = (Get-Item -LiteralPath $path).Length
    Set-SameSizeMutation -Path $path
    if ((Get-Item -LiteralPath $path).Length -ne $beforeLength) { throw "Mutation changed file size: $relative" }
    Invoke-ExpectedRejection -Fixture $fixture -Label "same-size changed $relative" -ExpectedDiagnostic 'Baseline file SHA-256 mismatch'
}

$missing = New-SyntheticTree -CaseName 'missing-required-file'
Remove-Item -LiteralPath (Join-Path $missing.BaselineRoot 'raster\result.xlsx')
Invoke-ExpectedRejection -Fixture $missing -Label 'missing required artifact' -ExpectedDiagnostic 'Unexpected or missing entry in baseline fixture'

$extra = New-SyntheticTree -CaseName 'extra-hidden-file'
$hidden = Join-Path $extra.BaselineRoot 'text-table\.unexpected'
[IO.File]::WriteAllText($hidden, 'synthetic hidden extra')
[IO.File]::SetAttributes($hidden, [IO.File]::GetAttributes($hidden) -bor [IO.FileAttributes]::Hidden)
if (([IO.File]::GetAttributes($hidden) -band [IO.FileAttributes]::Hidden) -eq 0) { throw 'Hidden-file fixture was not marked hidden.' }
Invoke-ExpectedRejection -Fixture $extra -Label 'extra hidden file' -ExpectedDiagnostic 'Unexpected or missing entry in baseline fixture'

foreach ($duplicate in @(
    [pscustomobject]@{ Name='duplicate-case-path'; Path='TEXT-TABLE/result.json' },
    [pscustomobject]@{ Name='duplicate-separator-path'; Path='text-table\result.json' }
)) {
    $fixture = New-SyntheticTree -CaseName $duplicate.Name
    $manifestObject = Get-Content -LiteralPath $fixture.ManifestPath -Raw | ConvertFrom-Json
    $original = @($manifestObject.files | Where-Object { $_.path -eq 'text-table/result.json' })[0]
    $manifestObject.files += [pscustomobject]@{ path=$duplicate.Path; size=$original.size; sha256=$original.sha256 }
    Write-ManifestFile -Path $fixture.ManifestPath -Manifest $manifestObject
    Invoke-ExpectedRejection -Fixture $fixture -Label $duplicate.Name -ExpectedDiagnostic 'Unexpected or unsafe baseline manifest relative path'
}

$traversal = New-SyntheticTree -CaseName 'traversal-path'
$manifestObject = Get-Content -LiteralPath $traversal.ManifestPath -Raw | ConvertFrom-Json
$manifestObject.files += [pscustomobject]@{ path='../outside.txt'; size=1; sha256=('0' * 64) }
Write-ManifestFile -Path $traversal.ManifestPath -Manifest $manifestObject
Invoke-ExpectedRejection -Fixture $traversal -Label 'manifest traversal path' -ExpectedDiagnostic 'Unexpected or unsafe baseline manifest relative path'

# A substituted root with plausible names but different bytes must not reach the comparator.
$original = New-SyntheticTree -CaseName 'substituted-alpha'
$substitute = New-SyntheticTree -CaseName 'substituted-bravo'
$substituteFixture = [pscustomobject]@{
    ManifestPath = $original.ManifestPath
    BaselineRoot = $substitute.BaselineRoot
    SnapshotRoot = $substitute.SnapshotRoot
}
Invoke-ExpectedRejection -Fixture $substituteFixture -Label 'substituted BaselineRoot' -ExpectedDiagnostic 'Baseline file SHA-256 mismatch'

$unsupported = New-SyntheticTree -CaseName 'unsupported-schema-version'
$manifestObject = Get-Content -LiteralPath $unsupported.ManifestPath -Raw | ConvertFrom-Json
$manifestObject.schema_version = 2
Write-ManifestFile -Path $unsupported.ManifestPath -Manifest $manifestObject
Invoke-ExpectedRejection -Fixture $unsupported -Label 'unsupported manifest schema version' -ExpectedDiagnostic 'Unsupported baseline manifest schema'

$badSize = New-SyntheticTree -CaseName 'invalid-size'
$manifestObject = Get-Content -LiteralPath $badSize.ManifestPath -Raw | ConvertFrom-Json
$manifestObject.files[0].size = -1
Write-ManifestFile -Path $badSize.ManifestPath -Manifest $manifestObject
Invoke-ExpectedRejection -Fixture $badSize -Label 'invalid manifest size' -ExpectedDiagnostic 'Invalid baseline file size'

$badHash = New-SyntheticTree -CaseName 'invalid-hash'
$manifestObject = Get-Content -LiteralPath $badHash.ManifestPath -Raw | ConvertFrom-Json
$manifestObject.files[0].sha256 = 'not-a-sha256'
Write-ManifestFile -Path $badHash.ManifestPath -Manifest $manifestObject
Invoke-ExpectedRejection -Fixture $badHash -Label 'invalid manifest hash' -ExpectedDiagnostic 'Invalid baseline file SHA-256'

$wrongManifestHash = New-SyntheticTree -CaseName 'manifest-hash-mismatch'
Invoke-ExpectedRejection -Fixture $wrongManifestHash -Label 'expected manifest hash mismatch' -ExpectedManifestSha256 ('f' * 64) -ExpectedDiagnostic 'Baseline manifest SHA-256 mismatch'

$junction = New-SyntheticTree -CaseName 'outside-junction' -JunctionTextTable
Invoke-ExpectedRejection -Fixture $junction -Label 'reparse-point baseline directory targets outside root' -ExpectedDiagnostic 'Reparse points are forbidden in baseline paths'

Write-Output 'Portable baseline synthetic tests PASS (no OCR runtime, build, private data, network, or production service used).'
Write-Output "Retained synthetic evidence: $runRoot"
