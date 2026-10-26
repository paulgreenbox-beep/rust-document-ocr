param(
    [Parameter(Mandatory = $true)][string]$PdfiumLibrary,
    [Parameter(Mandatory = $true)][string]$TessdataDirectory,
    [string]$ExpectedPdfiumSha256 = '79D4676B656CFB1ABCEA88F9ADE3B4B0826C5200382DB5F4EC72A636C598C118',
    [string]$ExpectedEngSha256 = '7D4322BD2A7749724879683FC3912CB542F19906C83BCC1A52132556427170B2',
    [string]$ExpectedRusSha256 = 'E16E5E036CCE1D9EC2B00063CF8B54472625B9E14D893A169E2B0DEDEB4DF225'
)
$ErrorActionPreference = 'Stop'
$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$runtimeRoot = Join-Path $projectRoot 'runtime'
$sources = @(
    @{ Source = $PdfiumLibrary; Destination = 'pdfium/7881/bin/pdfium.dll'; Expected = $ExpectedPdfiumSha256 },
    @{ Source = (Join-Path $TessdataDirectory 'eng.traineddata'); Destination = 'tessdata/eng.traineddata'; Expected = $ExpectedEngSha256 },
    @{ Source = (Join-Path $TessdataDirectory 'rus.traineddata'); Destination = 'tessdata/rus.traineddata'; Expected = $ExpectedRusSha256 },
    @{ Source = (Join-Path $projectRoot 'examples/documents/synthetic-text-table.pdf'); Destination = 'fixtures/w3c-dummy.pdf' }
)
foreach ($entry in $sources) {
    if (-not (Test-Path -LiteralPath $entry.Source -PathType Leaf)) {
        throw "Required external runtime/sample file is missing: $($entry.Source)"
    }
    if ($entry.Expected) {
        if ($entry.Expected -notmatch '^[0-9A-Fa-f]{64}$') { throw 'Expected SHA-256 must contain 64 hex characters' }
        $actualHash = (Get-FileHash -LiteralPath $entry.Source -Algorithm SHA256).Hash
        if ($actualHash -ne $entry.Expected) { throw "External runtime checksum mismatch: $($entry.Source)" }
    }
    $destination = [IO.Path]::GetFullPath((Join-Path $runtimeRoot $entry.Destination))
    if (-not $destination.StartsWith($runtimeRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Test runtime path is outside the project runtime directory'
    }
    if (Test-Path -LiteralPath $destination) {
        throw "Refusing to overwrite an existing runtime file: $destination"
    }
}
$libraryBytes = [IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $PdfiumLibrary).Path)
if ($libraryBytes.Length -lt 64) { throw 'PDFium library has no valid PE header' }
$peOffset = [BitConverter]::ToInt32($libraryBytes, 60)
if ($peOffset -lt 0 -or $peOffset -gt ($libraryBytes.Length - 6)) { throw 'Invalid PDFium PE offset' }
if ([BitConverter]::ToUInt32($libraryBytes, $peOffset) -ne 0x00004550 -or
    [BitConverter]::ToUInt16($libraryBytes, $peOffset + 4) -ne 0x8664) {
    throw 'PDFium must be a Windows x64 PE library'
}
foreach ($entry in $sources) {
    $destination = Join-Path $runtimeRoot $entry.Destination
    New-Item -ItemType Directory -Path (Split-Path -Parent $destination) -Force | Out-Null
    Copy-Item -LiteralPath $entry.Source -Destination $destination
}
Write-Output 'Prepared local ignored test runtime. No runtime assets belong in Git.'
