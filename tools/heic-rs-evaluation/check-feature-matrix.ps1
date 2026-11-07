[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$EvidenceRoot,
    [string]$TargetDir,
    [switch]$GraphOnly,
    [int]$Jobs = 2
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$project = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if (git -C $project status --porcelain=v1) { throw 'Feature matrix requires clean committed sources' }
if ($LASTEXITCODE -ne 0) { throw 'Cannot inspect source status' }
if ($Jobs -lt 1 -or $Jobs -gt 16) { throw 'Invalid job limit' }
$hostInfo = (& rustc -Vv | Out-String).Trim()
if ($LASTEXITCODE -ne 0) { throw 'Cannot identify Rust host' }
$hostTriple = [regex]::Match($hostInfo, '(?m)^host: (.+)$').Groups[1].Value.Trim()
if (!$GraphOnly -and $hostTriple -ne 'x86_64-pc-windows-msvc') {
    throw 'Native matrix compilation requires the Windows x64 MSVC host; use GraphOnly elsewhere'
}
if (Test-Path -LiteralPath $EvidenceRoot) { throw 'Fresh evidence root required' }
if (!$GraphOnly -and (!$TargetDir -or ![IO.Path]::IsPathFullyQualified($TargetDir))) {
    throw 'Compilation requires an explicit absolute research target directory'
}
$evidence = (New-Item -ItemType Directory -Path $EvidenceRoot).FullName
$matrix = @(
    @{ name = 'source-only'; flags = @('--no-default-features'); common = $false; legacy = $false; pure = $false },
    @{ name = 'legacy-native'; flags = @('--no-default-features', '--features', 'native'); common = $true; legacy = $true; pure = $false },
    @{ name = 'pure-heic-rs'; flags = @('--no-default-features', '--features', 'heic-rs-input'); common = $true; legacy = $false; pure = $true },
    @{ name = 'mixed'; flags = @('--no-default-features', '--features', 'native,heic-rs-input'); common = $true; legacy = $true; pure = $true },
    @{ name = 'all-features'; flags = @('--all-features'); common = $true; legacy = $true; pure = $true },
    @{ name = 'common-only'; flags = @('--no-default-features', '--features', 'native-common'); common = $true; legacy = $false; pure = $false }
)
$receipt = [ordered]@{
    schema_version = 1
    source_sha = (git -C $project rev-parse HEAD).Trim()
    git_tree_sha = (git -C $project rev-parse 'HEAD^{tree}').Trim()
    cargo_lock_sha256 = (Get-FileHash -LiteralPath (Join-Path $project 'Cargo.lock')).Hash.ToLowerInvariant()
    target = 'x86_64-pc-windows-msvc'
    rustc_host = $hostTriple
    rustc_version = $hostInfo
    graph_only = [bool]$GraphOnly
    cases = @()
}
Push-Location -LiteralPath $project
try {
    foreach ($case in $matrix) {
        $metadataArgs = @('metadata', '--offline', '--locked', '--format-version', '1', '--filter-platform', $receipt.target) + $case.flags
        $json = & cargo @metadataArgs 2> (Join-Path $evidence ($case.name + '.metadata.stderr.log'))
        if ($LASTEXITCODE -ne 0) { throw "Metadata failed: $($case.name)" }
        $metadata = ($json -join [Environment]::NewLine) | ConvertFrom-Json -Depth 100
        $selected = @{}
        foreach ($node in $metadata.resolve.nodes) { $selected[$node.id] = $node }
        $packages = @($metadata.packages | Where-Object { $selected.ContainsKey($_.id) })
        $names = @($packages.name)
        $rootNode = $selected[$metadata.resolve.root]
        $old = @($names | Where-Object { $_ -in @('libheif-rs', 'libheif-sys', 'libheif', 'libde265') })
        if ([bool]$old.Count -ne $case.legacy) { throw "Unexpected old HEIF graph: $($case.name)" }
        if (($names -contains 'heic-rs') -ne $case.pure) { throw "Unexpected Rust HEIC graph: $($case.name)" }
        if (($rootNode.features -contains 'native-common') -ne $case.common) { throw "Common pipeline gate mismatch: $($case.name)" }
        if ($case.name -eq 'pure-heic-rs' -and ($rootNode.features -contains 'native' -or $rootNode.features -contains 'heif-input')) {
            throw 'Pure configuration accidentally activates legacy native/heif-input'
        }
        $active = @($packages | Sort-Object name, version | ForEach-Object {
            @{ name = $_.name; version = $_.version; features = @($selected[$_.id].features | Sort-Object) }
        })
        $checkArgs = @('check', '--offline', '--locked') + $case.flags + @('--all-targets', '--target-dir', $TargetDir, '-j', "$Jobs")
        $status = 'NOT_RUN_GRAPH_ONLY'
        if (!$GraphOnly) {
            & cargo @checkArgs *> (Join-Path $evidence ($case.name + '.check.log'))
            if ($LASTEXITCODE -ne 0) { throw "All-targets check failed: $($case.name)" }
            $status = 'PASS'
        }
        $receipt.cases += @{ name = $case.name; metadata_command = @('cargo') + $metadataArgs;
            compile_command = if ($GraphOnly) { $null } else { @('cargo') + $checkArgs };
            compilation = $status; selected_root_features = @($rootNode.features | Sort-Object);
            old_heif_crates = $old; selected_packages = $active }
        Write-Output "$($case.name): graph PASS, compilation $status"
    }
    if ((Get-FileHash -LiteralPath 'Cargo.lock').Hash.ToLowerInvariant() -ne $receipt.cargo_lock_sha256) {
        throw 'Lockfile changed during locked matrix verification'
    }
    if ((git rev-parse HEAD).Trim() -ne $receipt.source_sha -or
        (git rev-parse 'HEAD^{tree}').Trim() -ne $receipt.git_tree_sha -or
        (git status --porcelain=v1)) {
        throw 'Source HEAD/tree or working files changed during matrix verification'
    }
    [IO.File]::WriteAllText((Join-Path $evidence 'feature-matrix.json'), ($receipt | ConvertTo-Json -Depth 20))
} finally { Pop-Location }
