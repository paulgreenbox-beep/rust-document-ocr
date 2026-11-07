# Portable OCR comparison consumes exactly these files, not an arbitrary tree.
$portableBaselineFixtures=@('text-table','raster')
$portableBaselineOutputs=@('result.json','result.v1.json','result.docx','result.xlsx')

function Assert-PortableBaselinePath([string]$Path) {
    # Walk from the volume root so an ancestor junction is rejected before
    # inspecting children through it.
    $paths=[Collections.Generic.Stack[string]]::new()
    for ($walk=$Path; $walk; $walk=[IO.Path]::GetDirectoryName($walk)) { $paths.Push($walk) }
    foreach ($part in $paths) {
        if ([IO.File]::GetAttributes($part) -band [IO.FileAttributes]::ReparsePoint) {
            throw 'Reparse points are forbidden in baseline paths'
        }
    }
}

function Get-PortableBaselineJsonFields([System.Text.Json.JsonElement]$Element) {
    if ($Element.ValueKind -ne 'Object') { throw 'Baseline manifest must contain JSON objects' }
    $fields=[Collections.Generic.Dictionary[string,System.Text.Json.JsonElement]]::new([StringComparer]::Ordinal)
    foreach ($property in $Element.EnumerateObject()) {
        if (!$fields.TryAdd($property.Name,$property.Value)) { throw 'Duplicate baseline manifest JSON key' }
    }
    return ,$fields
}

function Get-PortableBaselineEvidence {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$ManifestPath,
        [Parameter(Mandatory)][string]$BaselineRoot,
        [string]$ExpectedManifestSha256,
        [switch]$RequireVersionedManifest,
        [string]$SnapshotRoot,
        [Collections.Generic.List[IO.FileStream]]$SnapshotHandles
    )
    $ErrorActionPreference='Stop'
    $manifest=[IO.Path]::GetFullPath($ManifestPath)
    $root=[IO.Path]::TrimEndingDirectorySeparator([IO.Path]::GetFullPath($BaselineRoot))
    foreach ($path in @($manifest,$root)) {
        if ($path -match '^[\\/]{2}[?.][\\/]') { throw 'Device paths are forbidden in baseline inputs' }
        Assert-PortableBaselinePath $path
    }
    if (![IO.File]::Exists($manifest) -or ![IO.Directory]::Exists($root)) { throw 'Baseline manifest file and baseline directory are required' }
    $bytes=[IO.File]::ReadAllBytes($manifest)
    $manifestSha=[Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($bytes))
    if ($ExpectedManifestSha256 -and $manifestSha -ne $ExpectedManifestSha256) { throw 'Baseline manifest SHA-256 mismatch' }
    $json=[Text.UTF8Encoding]::new($false,$true).GetString($bytes).TrimStart([char]0xfeff)
    $document=[System.Text.Json.JsonDocument]::Parse($json)
    try {
        $fields=Get-PortableBaselineJsonFields $document.RootElement
        $versioned=$fields.ContainsKey('schema_version')
        if ($versioned) {
            $version=$fields['schema_version']
            if ($fields.Count -ne 2 -or $version.ValueKind -ne 'Number' -or $version.GetRawText() -cne '1') { throw 'Unsupported baseline manifest schema' }
        } else {
            # Only the observed unversioned RC1 receipt shape is supported for
            # local comparison. It binds hashes, but has no recorded sizes.
            if ($RequireVersionedManifest) { throw 'ZIP requires baseline manifest schema_version 1 with sizes' }
            if ($fields.Count -ne 4 -or !$fields.ContainsKey('scope') -or $fields['scope'].ValueKind -ne 'String') { throw 'Invalid legacy baseline manifest' }
            foreach ($key in @('exe_sha256','runtime_manifest_sha256')) {
                if (!$fields.ContainsKey($key) -or $fields[$key].ValueKind -ne 'String' -or $fields[$key].GetString() -notmatch '\A[0-9a-f]{64}\z') { throw 'Invalid legacy baseline provenance' }
            }
        }
        if (!$fields.ContainsKey('files') -or $fields['files'].ValueKind -ne 'Array') { throw 'Missing baseline manifest files array' }
        $expected=@(foreach ($fixture in $portableBaselineFixtures) { foreach ($name in $portableBaselineOutputs) { "$fixture/$name" } })
        $records=@{}
        foreach ($entry in $fields['files'].EnumerateArray()) {
            $record=Get-PortableBaselineJsonFields $entry
            $fieldCount=if ($versioned) { 3 } else { 2 }
            if ($record.Count -ne $fieldCount -or !$record.ContainsKey('path') -or !$record.ContainsKey('sha256') -or $record['path'].ValueKind -ne 'String' -or $record['sha256'].ValueKind -ne 'String') { throw 'Invalid baseline manifest file record' }
            $relative=$record['path'].GetString()
            $hash=$record['sha256'].GetString()
            if ($relative -cnotin $expected) { throw 'Unexpected or unsafe baseline manifest relative path' }
            if ($records.ContainsKey($relative)) { throw 'Duplicate baseline manifest relative path' }
            if ($hash -notmatch '\A[0-9a-f]{64}\z') { throw 'Invalid baseline file SHA-256' }
            $size=0L
            if ($versioned -and (!$record.ContainsKey('size') -or $record['size'].ValueKind -ne 'Number' -or !$record['size'].TryGetInt64([ref]$size) -or $size -le 0)) { throw 'Invalid baseline file size' }
            $records[$relative]=@{Sha256=$hash;Size=$size}
        }
        if ($records.Count -ne $expected.Count) { throw 'Incomplete baseline manifest file set' }
    } finally { $document.Dispose() }

    # Enumerate without recursion and without following links; include hidden
    # entries. Exact names exclude case/separator aliases and NTFS ADS syntax.
    $directories=[IO.DirectoryInfo]::new($root).GetFileSystemInfos()
    if ($directories.Count -ne $portableBaselineFixtures.Count) { throw 'Unexpected or missing entry in baseline root' }
    foreach ($directory in $directories) {
        if ($directory.Name -cnotin $portableBaselineFixtures -or $directory -isnot [IO.DirectoryInfo]) { throw 'Unexpected entry in baseline root' }
        Assert-PortableBaselinePath $directory.FullName
        $children=$directory.GetFileSystemInfos()
        if ($children.Count -ne $portableBaselineOutputs.Count) { throw 'Unexpected or missing entry in baseline fixture' }
        foreach ($child in $children) {
            if ($child.Name -cnotin $portableBaselineOutputs -or $child -isnot [IO.FileInfo]) { throw 'Unexpected entry in baseline fixture' }
            Assert-PortableBaselinePath $child.FullName
        }
    }

    if ($SnapshotRoot) {
        if ($null -eq $SnapshotHandles) { throw 'Baseline snapshot must retain read-only file handles' }
        $SnapshotRoot=[IO.Path]::GetFullPath($SnapshotRoot)
        Assert-PortableBaselinePath ([IO.Path]::GetDirectoryName($SnapshotRoot))
        if (Test-Path -LiteralPath $SnapshotRoot) { throw 'Baseline snapshot path must be new' }
        [IO.Directory]::CreateDirectory($SnapshotRoot) | Out-Null
        Assert-PortableBaselinePath $SnapshotRoot
        foreach ($fixture in $portableBaselineFixtures) { [IO.Directory]::CreateDirectory((Join-Path $SnapshotRoot $fixture)) | Out-Null }
    }
    foreach ($relative in $expected) {
        $path=Join-Path $root $relative
        Assert-PortableBaselinePath $path
        $source=[IO.File]::Open($path,'Open','Read','Read')
        try {
            $record=$records[$relative]
            if ($source.Length -le 0 -or ($versioned -and $source.Length -ne $record.Size)) { throw "Baseline file size mismatch: $relative" }
            if ($SnapshotRoot) {
                $snapshotPath=Join-Path $SnapshotRoot $relative
                Assert-PortableBaselinePath ([IO.Path]::GetDirectoryName($snapshotPath))
                $output=[IO.File]::Open($snapshotPath,'CreateNew','Write','None')
                try { $source.CopyTo($output) } finally { $output.Dispose() }
                Assert-PortableBaselinePath $snapshotPath
                # Hash the actual comparison copy and prevent ordinary Windows
                # writes/deletes from replacing it during the comparison.
                $snapshot=[IO.File]::Open($snapshotPath,'Open','Read','Read')
                $SnapshotHandles.Add($snapshot)
                if ($snapshot.Length -ne $source.Length) { throw "Baseline snapshot size mismatch: $relative" }
                $hash=[Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($snapshot))
            } else { $hash=[Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($source)) }
            if ($hash -ne $record.Sha256) { throw "Baseline file SHA-256 mismatch: $relative" }
        } finally { $source.Dispose() }
    }
    return @{ManifestSha256=$manifestSha;Versioned=$versioned}
}

function Invoke-PortableBaselineComparison {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$ManifestPath,
        [Parameter(Mandatory)][string]$BaselineRoot,
        [Parameter(Mandatory)][string]$SnapshotRoot,
        [Parameter(Mandatory)][scriptblock]$Compare,
        [string]$ExpectedManifestSha256,
        [switch]$RequireVersionedManifest
    )
    $ErrorActionPreference='Stop'
    $handles=[Collections.Generic.List[IO.FileStream]]::new()
    try {
        $null=Get-PortableBaselineEvidence -ManifestPath $ManifestPath -BaselineRoot $BaselineRoot -SnapshotRoot $SnapshotRoot -SnapshotHandles $handles -ExpectedManifestSha256 $ExpectedManifestSha256 -RequireVersionedManifest:$RequireVersionedManifest
        & $Compare ([IO.Path]::GetFullPath($SnapshotRoot))
    } finally {
        foreach ($handle in $handles) { $handle.Dispose() }
    }
}
