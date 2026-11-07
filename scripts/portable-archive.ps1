# Internal Windows portable ZIP publication. Dot-source from the packager.

function Assert-PortableArchiveName([string]$Name) {
    if (!$Name -or $Name -match '[\\:\x00-\x1f<>"|?*]') { throw "Unsafe ZIP path: $Name" }
    foreach ($part in $Name.Split('/')) {
        if (!$part -or $part -match '[. ]$' -or ($part.Split('.')[0].TrimEnd(' ')) -match '^(?i:CON|PRN|AUX|NUL|CONIN\$|CONOUT\$|COM[1-9\u00b9\u00b2\u00b3]|LPT[1-9\u00b9\u00b2\u00b3])$') {
            throw "Unsafe ZIP path: $Name"
        }
    }
}

function Get-PortableArchiveItem([string]$Path) {
    try { Get-Item -LiteralPath $Path -Force -ErrorAction Stop }
    catch [System.Management.Automation.ItemNotFoundException] { return }
}

function Assert-PortableArchiveSafeParents([string]$Path) {
    if ($Path.StartsWith('\\?\',[StringComparison]::Ordinal) -or $Path.StartsWith('\\.\',[StringComparison]::Ordinal) -or
        $Path.StartsWith('\??\',[StringComparison]::Ordinal)) { throw "ZIP device path is not allowed: $Path" }
    # Check from the drive root down, so Get-Item never follows a discovered junction.
    $parents=[Collections.Generic.Stack[string]]::new()
    for ($parent=[IO.Directory]::GetParent($Path); $null -ne $parent; $parent=$parent.Parent) {
        $parents.Push($parent.FullName)
    }
    foreach ($parentPath in $parents) {
        $item=Get-PortableArchiveItem $parentPath
        if ($null -ne $item -and (!$item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint))) {
            throw "ZIP parent must be an ordinary directory: $parentPath"
        }
    }
}

function Assert-PortableArchiveDestination {
    param([Parameter(Mandatory)][string]$FinalPath)
    $final=[IO.Path]::GetFullPath($FinalPath)
    $leaf=[IO.Path]::GetFileName($final)
    Assert-PortableArchiveName $leaf
    if ([IO.Path]::GetExtension($final) -ine '.zip') { throw 'Final archive must have a .zip extension' }
    Assert-PortableArchiveSafeParents $final
    if ($null -ne (Get-PortableArchiveItem $final)) { throw "Final ZIP exists; refusing overwrite: $final" }
}

function Get-PortableArchiveTree([string]$PackageRoot) {
    $root=[IO.Path]::TrimEndingDirectorySeparator([IO.Path]::GetFullPath($PackageRoot))
    Assert-PortableArchiveSafeParents $root
    $item=Get-Item -LiteralPath $root -Force -ErrorAction Stop
    $forbidden=[IO.FileAttributes]::ReparsePoint -bor [IO.FileAttributes]::Hidden -bor [IO.FileAttributes]::System -bor [IO.FileAttributes]::Device
    if (!$item.PSIsContainer -or ($item.Attributes -band $forbidden)) { throw "Not an ordinary package directory: $root" }
    Assert-PortableArchiveName $item.Name
    $files=[Collections.Generic.Dictionary[string,object]]::new([StringComparer]::OrdinalIgnoreCase)
    $dirs=[Collections.Generic.Dictionary[string,string]]::new([StringComparer]::OrdinalIgnoreCase)
    $walk=[Collections.Generic.Stack[string]]::new()
    $walk.Push($root)
    while ($walk.Count) {
        $directory=$walk.Pop()
        # -Force is essential: an unlisted hidden file cannot disappear from the inventory.
        foreach ($child in Get-ChildItem -LiteralPath $directory -Force -ErrorAction Stop) {
            $name=$child.FullName.Substring($root.Length+1).Replace('\','/')
            Assert-PortableArchiveName $name
            if ($child.Attributes -band $forbidden) { throw "Reparse, hidden, system or device package entry: $name" }
            if ($dirs.ContainsKey($name) -or $files.ContainsKey($name)) { throw "Ambiguous package entry: $name" }
            if ($child.PSIsContainer) {
                $dirs.Add($name,$name)
                $walk.Push($child.FullName)
            } else {
                $files.Add($name,[pscustomobject]@{Name=$name; Size=$child.Length; Hash=(Get-FileHash -LiteralPath $child.FullName -Algorithm SHA256 -ErrorAction Stop).Hash})
            }
        }
    }
    return [pscustomobject]@{Root=$root; Leaf=$item.Name; Files=$files; Directories=$dirs}
}

function Get-PortableArchiveSnapshot {
    param([Parameter(Mandatory)][string]$PackageRoot,[Parameter(Mandatory)][string]$ExpectedManifestSha256)
    $tree=Get-PortableArchiveTree $PackageRoot
    $manifestName='release-manifest.json'
    if (!$tree.Files.ContainsKey($manifestName) -or $tree.Files[$manifestName].Name -cne $manifestName) {
        throw 'Missing canonical release-manifest.json before ZIP compression'
    }
    # Parse exactly the manifest bytes whose hash is in the trusted staged snapshot.
    $path=Join-Path $tree.Root $manifestName
    $bytes=[IO.File]::ReadAllBytes($path)
    $manifestHash=[Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($bytes))
    if ($manifestHash -cne $tree.Files[$manifestName].Hash -or $manifestHash -ine $ExpectedManifestSha256) {
        throw 'Release manifest changed while taking the staged snapshot'
    }
    $manifest=[Text.Encoding]::UTF8.GetString($bytes).TrimStart([char]0xfeff) | ConvertFrom-Json -Depth 100 -ErrorAction Stop
    if ($manifest.schema_version -ne 1 -or $manifest.files -isnot [array] -or !$manifest.files.Count) { throw 'Invalid/empty staged release manifest' }
    $listed=[Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    foreach ($entry in $manifest.files) {
        if ($entry.path -isnot [string]) { throw 'Invalid staged manifest path type' }
        $name=$entry.path
        Assert-PortableArchiveName $name
        if ($name -ieq $manifestName -or !$listed.Add($name) -or !$tree.Files.ContainsKey($name) -or $tree.Files[$name].Name -cne $name) {
            throw "Missing or ambiguous staged manifest file: $name"
        }
        $file=$tree.Files[$name]
        if (($entry.size -isnot [long] -and $entry.size -isnot [int]) -or $entry.size -ne $file.Size -or
            $entry.sha256 -isnot [string] -or $entry.sha256 -notmatch '^[0-9a-fA-F]{64}$' -or $entry.sha256 -ine $file.Hash) {
            throw "Staged release manifest integrity failure: $name"
        }
    }
    if ($tree.Files.Count -ne ($listed.Count+1)) { throw 'Unexpected staged package file not listed in release manifest' }
    return $tree
}

function Assert-PortableArchiveTree($Expected, [string]$PackageRoot) {
    $actual=Get-PortableArchiveTree $PackageRoot
    if ($actual.Leaf -cne $Expected.Leaf -or $actual.Files.Count -ne $Expected.Files.Count -or $actual.Directories.Count -ne $Expected.Directories.Count) {
        throw 'Extracted ZIP tree inventory does not match the staged package'
    }
    foreach ($name in $Expected.Directories.Keys) {
        if (!$actual.Directories.ContainsKey($name) -or $actual.Directories[$name] -cne $name) { throw "Unexpected ZIP directory inventory: $name" }
    }
    foreach ($name in $Expected.Files.Keys) {
        if (!$actual.Files.ContainsKey($name)) { throw "Missing extracted ZIP file: $name" }
        $file=$actual.Files[$name]; $expectedFile=$Expected.Files[$name]
        if ($file.Name -cne $name -or $file.Size -ne $expectedFile.Size -or $file.Hash -cne $expectedFile.Hash) {
            throw "Extracted ZIP integrity failure: $name"
        }
    }
}

function Write-PortablePendingZip($Snapshot, [string]$PendingPath) {
    $output=[IO.File]::Open($PendingPath,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::None)
    try {
        $zip=[IO.Compression.ZipArchive]::new($output,[IO.Compression.ZipArchiveMode]::Create,$true,[Text.Encoding]::UTF8)
        try {
            $zip.CreateEntry($Snapshot.Leaf+'/').ExternalAttributes=[int][IO.FileAttributes]::Directory
            foreach ($name in ($Snapshot.Directories.Keys | Sort-Object -CaseSensitive)) {
                $zip.CreateEntry($Snapshot.Leaf+'/'+$name+'/').ExternalAttributes=[int][IO.FileAttributes]::Directory
            }
            foreach ($name in ($Snapshot.Files.Keys | Sort-Object -CaseSensitive)) {
                $entry=$zip.CreateEntry($Snapshot.Leaf+'/'+$name,[IO.Compression.CompressionLevel]::Optimal)
                $entry.ExternalAttributes=0
                $source=[IO.File]::Open((Join-Path $Snapshot.Root $name),[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read)
                try {
                    $target=$entry.Open()
                    try { $source.CopyTo($target) } finally { $target.Dispose() }
                } finally { $source.Dispose() }
            }
        } finally { $zip.Dispose() }
    } finally { $output.Dispose() }
}

function Assert-PortableZipEntries($Zip, $Snapshot) {
    $seen=[Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    $prefix=$Snapshot.Leaf+'/'
    foreach ($entry in $Zip.Entries) {
        $full=$entry.FullName
        $directory=$full.EndsWith('/')
        $name=if ($directory) { $full.Substring(0,$full.Length-1) } else { $full }
        Assert-PortableArchiveName $name
        if (!$seen.Add($name)) { throw "Duplicate or ambiguous ZIP entry: $full" }
        if ($directory) {
            if ($entry.ExternalAttributes -ne [int][IO.FileAttributes]::Directory -or $entry.Length -ne 0 -or $entry.CompressedLength -ne 0) { throw "Invalid ZIP directory entry: $full" }
            if ($full.Equals($prefix,[StringComparison]::Ordinal)) { continue }
        }
        if (!$full.StartsWith($prefix,[StringComparison]::Ordinal)) { throw "Unexpected ZIP top-level path: $full" }
        $relative=$name.Substring($prefix.Length)
        if ($directory) {
            if (!$Snapshot.Directories.ContainsKey($relative) -or $Snapshot.Directories[$relative] -cne $relative) { throw "Unexpected ZIP directory: $full" }
        } else {
            if ($entry.ExternalAttributes -ne 0) { throw "Unexpected ZIP file attributes: $full" }
            if (!$Snapshot.Files.ContainsKey($relative) -or $Snapshot.Files[$relative].Name -cne $relative -or $entry.Length -ne $Snapshot.Files[$relative].Size) {
                throw "Unexpected ZIP file name or size: $full"
            }
        }
    }
    if ($seen.Count -ne ($Snapshot.Files.Count+$Snapshot.Directories.Count+1)) { throw 'ZIP entries are missing from the staged inventory' }
}

function Expand-PortablePendingZip($Zip, [string]$ExtractionPath) {
    New-Item -ItemType Directory -Path $ExtractionPath -ErrorAction Stop | Out-Null
    foreach ($entry in $Zip.Entries) {
        $path=Join-Path $ExtractionPath $entry.FullName
        if ($entry.FullName.EndsWith('/')) {
            [IO.Directory]::CreateDirectory($path) | Out-Null
            continue
        }
        [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($path)) | Out-Null
        $target=[IO.File]::Open($path,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::None)
        try {
            $source=$entry.Open()
            try {
                # Enforce the bound during decompression, even if a corrupt ZIP lies in its directory.
                $buffer=[byte[]]::new(65536); $remaining=$entry.Length
                while (($read=$source.Read($buffer,0,$buffer.Length)) -gt 0) {
                    $remaining-=$read
                    if ($remaining -lt 0) { throw "ZIP decompressed past the expected size: $($entry.FullName)" }
                    $target.Write($buffer,0,$read)
                }
                if ($remaining -ne 0) { throw "ZIP decompressed short of the expected size: $($entry.FullName)" }
            } finally { $source.Dispose() }
        } finally { $target.Dispose() }
    }
}

function Move-PortablePendingZip([string]$PendingPath, [string]$FinalPath) {
    # Both names are in one directory. This is a same-volume rename, without overwrite.
    [IO.File]::Move($PendingPath,$FinalPath)
}

function Publish-PortableArchive {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$PackageRoot,
        [Parameter(Mandatory)][string]$FinalPath,
        [Parameter(Mandatory)][ValidatePattern('^[0-9a-fA-F]{64}$')][string]$ExpectedManifestSha256,
        [Parameter(Mandatory)][scriptblock]$Smoke
    )
    $ErrorActionPreference='Stop'
    Assert-PortableArchiveDestination -FinalPath $FinalPath
    $final=[IO.Path]::GetFullPath($FinalPath)
    $parent=[IO.Path]::GetDirectoryName($final)
    $snapshot=Get-PortableArchiveSnapshot -PackageRoot $PackageRoot -ExpectedManifestSha256 $ExpectedManifestSha256
    if (![string]::Equals([IO.Path]::GetDirectoryName($snapshot.Root),$parent,[StringComparison]::OrdinalIgnoreCase)) {
        throw 'Final ZIP must be a sibling of the staged package'
    }
    $id=[guid]::NewGuid().ToString('N')
    $pending=Join-Path $parent ('.portable-'+$id+'.zip.pending')
    $extract=Join-Path $parent ('zip-smoke-'+$id)
    try {
        Write-PortablePendingZip $snapshot $pending
        # Keep the same archive open without write/delete sharing through extraction and smoke.
        $inputZip=[IO.File]::Open($pending,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read)
        try {
            $hash=(Get-FileHash -InputStream $inputZip -Algorithm SHA256).Hash
            $inputZip.Position=0
            $zip=[IO.Compression.ZipArchive]::new($inputZip,[IO.Compression.ZipArchiveMode]::Read,$true,[Text.Encoding]::UTF8)
            try {
                Assert-PortableZipEntries $zip $snapshot
                Expand-PortablePendingZip $zip $extract
            } finally { $zip.Dispose() }
            $extractedRoot=Join-Path $extract $snapshot.Leaf
            Assert-PortableArchiveTree $snapshot $extractedRoot
            & $Smoke $extractedRoot | Out-Host
            Assert-PortableArchiveTree $snapshot $extractedRoot
            $inputZip.Position=0
            if ((Get-FileHash -InputStream $inputZip -Algorithm SHA256).Hash -cne $hash) { throw 'Pending ZIP bytes changed during validation' }
            Assert-PortableArchiveSafeParents $final
            # Prepare the checksum of the validated bytes but emit it only after successful rename.
            $receipt=[pscustomobject]@{Algorithm='SHA256';Hash=$hash;Path=$final}
        } finally { $inputZip.Dispose() }
        Move-PortablePendingZip $pending $final
        return $receipt
    } catch {
        throw "Portable ZIP publication failed: $($_.Exception.Message)`nStaged package: $($snapshot.Root)`nPending archive (if created): $pending`nExtraction evidence (if created): $extract"
    }
}
