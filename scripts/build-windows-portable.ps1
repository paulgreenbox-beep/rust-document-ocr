[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$RuntimeManifest,
    [Parameter(Mandatory)][string]$Destination,
    [Parameter(Mandatory)][string]$ApprovedOutputRoot,
    [switch]$CreateZip,
    [switch]$SkipBuild,
    [string]$BuildReceipt,
    [string]$CleanEnvironmentReceipt,
    [string]$BaselineManifest,
    [string]$BaselineRoot,
    [string]$PythonPath
)
$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
$repo=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
. (Join-Path $PSScriptRoot 'portable-pe.ps1')
$dest=[IO.Path]::GetFullPath($Destination)
$approved=[IO.Path]::GetFullPath($ApprovedOutputRoot).TrimEnd('\')
if (!$dest.StartsWith($approved+'\',[StringComparison]::OrdinalIgnoreCase) -or $dest.StartsWith($repo+'\',[StringComparison]::OrdinalIgnoreCase) -or $dest -eq $repo) { throw 'Destination must be outside sources, strictly below ApprovedOutputRoot' }
if (Test-Path -LiteralPath $dest) { throw 'Destination exists; refusing overwrite' }
$m=Get-Content -LiteralPath $RuntimeManifest -Raw | ConvertFrom-Json
if ($m.schema_version -ne 1) { throw 'Unsupported runtime manifest schema' }
if (!($m.PSObject.Properties.Name -contains 'static_components') -or @($m.static_components).Count -eq 0) { throw 'Static component inventory is mandatory (Cargo + native static libraries)' }
foreach ($c in @($m.components)+@($m.static_components)) {
    foreach ($key in @('id','version','source_url','license','distribution_status','license_review','notices')) {
        if (!($c.PSObject.Properties.Name -contains $key) -or ($key -ne 'notices' -and !$c.$key)) { throw "Missing component metadata $key" }
    }
    if ($CreateZip -and ($c.distribution_status -ne 'verified' -or $c.license_review -notmatch '\S')) { throw "Unverified distribution rights: $($c.id)" }
    if ($CreateZip -and @($c.notices).Count -eq 0) { throw "Missing reviewed distribution notices: $($c.id)" }
    foreach ($n in $c.notices) { if ($n -notmatch '^licenses[/\\][^:]+' -or $n -match '(^|[\\/])\.\.([\\/]|$)') { throw 'Notices must be safe paths under licenses/' } }
}
foreach ($c in $m.static_components) {
    if (!($c.PSObject.Properties.Name -contains 'sha256') -or $c.sha256 -notmatch '^[A-Fa-f0-9]{64}$') { throw "Static component requires exact source/artifact SHA-256: $($c.id)" }
}
if ($CreateZip -and ($m.distribution_status -ne 'verified' -or !$BaselineManifest -or !$BaselineRoot -or !$PythonPath)) { throw 'ZIP requires verified global rights, accepted baseline manifest/tree and maintainer Python' }
if ($CreateZip -and !$CleanEnvironmentReceipt) { throw 'ZIP requires an independently recorded clean Windows x64 smoke receipt' }
$seen=@{}
foreach ($c in $m.components) { foreach ($f in $c.files) {
    $key=$f.path.Replace('\','/').ToLowerInvariant()
    if ($key -match '(^/|:|//|(^|/)\.{1,2}(/|$)|[. ](/|$))' -or $key -match '(^|/)tesseract\.exe$') { throw "Unsafe/unneeded payload path: $($f.path)" }
    if ($key -in @('ocr-pipeline.exe','release-manifest.json','verify.ps1','quick_start.md','troubleshooting.md','licenses/mit.txt','licenses/third_party_notices.md') -or $key.StartsWith('examples/')) { throw 'Runtime manifest cannot replace project-owned files' }
    if ($seen.ContainsKey($key)) { throw "Duplicate payload path: $($f.path)" }; $seen[$key]=$true
    if ($f.sha256 -notmatch '^[a-fA-F0-9]{64}$' -or (Get-FileHash -LiteralPath $f.source -Algorithm SHA256).Hash -ne $f.sha256) { throw "Runtime hash mismatch: $($f.path)" }
} }
Push-Location $repo
try {
    if (!$SkipBuild) { & cargo build --release --offline --locked --features native --bin ocr-pipeline -j 1; if ($LASTEXITCODE) { throw 'Native release build failed' } }
    $exe=Join-Path $repo 'target/release/ocr-pipeline.exe'
    if (!(Test-Path -LiteralPath $exe)) { throw 'Native executable missing' }
    if ($CreateZip) {
        $clean=Get-Content -LiteralPath $CleanEnvironmentReceipt -Raw | ConvertFrom-Json
        if ($clean.status -ne 'PASS' -or $clean.environment -ne 'clean-windows-x64' -or $clean.exe_sha256 -ne (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash -or $clean.baseline_sha256 -ne (Get-FileHash -LiteralPath $BaselineManifest -Algorithm SHA256).Hash -or $clean.runtime_manifest_sha256 -ne (Get-FileHash -LiteralPath $RuntimeManifest -Algorithm SHA256).Hash) { throw 'Clean Windows receipt must bind exact executable, runtime inventory and accepted baseline' }
    }
    if ($SkipBuild) {
        if (!$BuildReceipt) { throw 'SkipBuild requires a hash-bound native build receipt' }
        $receipt=Get-Content -LiteralPath $BuildReceipt -Raw | ConvertFrom-Json
        if (!$receipt.native -or !$receipt.locked -or !$receipt.offline -or $receipt.sha256 -ne (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash) { throw 'Invalid native build receipt' }
    }
    $version=[regex]::Match((Get-Content Cargo.toml -Raw),'(?m)^version\s*=\s*"([^"]+)"').Groups[1].Value
    New-Item -ItemType Directory -Path $dest | Out-Null
    Copy-Item -LiteralPath $exe -Destination (Join-Path $dest 'ocr-pipeline.exe')
    foreach ($c in $m.components) { foreach ($f in $c.files) {
        $to=Join-Path $dest $f.path
        New-Item -ItemType Directory -Path (Split-Path $to -Parent) -Force | Out-Null
        Copy-Item -LiteralPath $f.source -Destination $to
    } }
    foreach ($required in @('pdfium.dll','tessdata/eng.traineddata','tessdata/rus.traineddata')) { if (!(Test-Path -LiteralPath (Join-Path $dest $required))) { throw "Missing required payload: $required" } }
    foreach ($c in @($m.components)+@($m.static_components)) { foreach ($n in $c.notices) { if (!(Test-Path -LiteralPath (Join-Path $dest $n) -PathType Leaf)) { throw "Missing notice: $n" } } }
    New-Item -ItemType Directory -Path (Join-Path $dest 'licenses'),(Join-Path $dest 'examples') -Force | Out-Null
    Copy-Item LICENSE (Join-Path $dest 'licenses/MIT.txt')
    Copy-Item THIRD_PARTY_NOTICES.md (Join-Path $dest 'licenses/THIRD_PARTY_NOTICES.md')
    # Never glob arbitrary documents into a binary package. Only the two
    # reviewed fixtures with committed checksums are permitted.
    $fixtureSums=@(Get-Content -LiteralPath (Join-Path $repo 'examples/documents/SHA256SUMS'))
    foreach ($fixture in @('synthetic-text-table.pdf','synthetic-raster.pdf')) {
        $records=@($fixtureSums | Where-Object { $_ -match ('^[a-f0-9]{64}\s+'+[regex]::Escape($fixture)+'$') })
        if ($records.Count -ne 1) { throw "Missing/ambiguous reviewed fixture checksum: $fixture" }
        $expected=($records[0] -split '\s+',2)[0]
        $fixturePath=Join-Path $repo "examples/documents/$fixture"
        if ((Get-FileHash -LiteralPath $fixturePath -Algorithm SHA256).Hash -ne $expected) { throw "Reviewed fixture hash mismatch: $fixture" }
        Copy-Item -LiteralPath $fixturePath -Destination (Join-Path $dest 'examples')
    }
    Copy-Item deploy/windows-portable/* $dest
    $closure=@(Test-PortablePeClosure $dest)
    $files=@(Get-ChildItem -LiteralPath $dest -Recurse -File | ForEach-Object { @{path=$_.FullName.Substring($dest.Length+1).Replace('\','/');sha256=(Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash;size=$_.Length} })
    # Never publish local developer paths in the distributable inventory.
    $publicComponents=@($m.components | ForEach-Object {
        $component=$_ | ConvertTo-Json -Depth 25 | ConvertFrom-Json
        foreach ($file in $component.files) { $file.PSObject.Properties.Remove('source') }
        $component
    })
    $head=(& git rev-parse HEAD).Trim()
    if ($LASTEXITCODE) { throw 'Cannot record git HEAD' }
    $dirtyEntries=@(& git status --porcelain --untracked-files=normal)
    if ($LASTEXITCODE) { throw 'Cannot record git dirty status' }
    $toolchain=@{rustc_version='NOT_RECORDED';cargo_version='NOT_RECORDED';source='No toolchain version receipt supplied; no version is inferred from an installed executable.'}
    $toolReceiptPath=$BuildReceipt
    if ($toolReceiptPath) {
        $toolReceipt=Get-Content -LiteralPath $toolReceiptPath -Raw | ConvertFrom-Json
        foreach ($name in @('rustc_version','cargo_version')) {
            if ($toolReceipt.PSObject.Properties.Name -contains $name -and $toolReceipt.$name) { $toolchain[$name]=$toolReceipt.$name }
        }
        $toolchain.source='Caller-supplied build/toolchain receipt; versions are evidence supplied by the build operator.'
        $toolchain.receipt_sha256=(Get-FileHash -LiteralPath $toolReceiptPath -Algorithm SHA256).Hash
    }
    elseif (!$SkipBuild) {
        $toolchain.rustc_version=(& rustc --version | Out-String).Trim()
        if ($LASTEXITCODE) { throw 'Cannot record rustc version after build' }
        $toolchain.cargo_version=(& cargo --version | Out-String).Trim()
        if ($LASTEXITCODE) { throw 'Cannot record Cargo version after build' }
        $toolchain.source='Version commands captured after the native build in this invocation.'
    }
    $sourceFiles=@(Get-ChildItem -LiteralPath (Join-Path $repo 'src') -Recurse -File -Filter '*.rs' | Sort-Object FullName | ForEach-Object {
        @{path=$_.FullName.Substring($repo.Length+1).Replace('\','/');sha256=(Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash}
    })
    $provenance=@{git_head=$head;git_dirty=($dirtyEntries.Count -gt 0);cargo_lock_sha256=(Get-FileHash -LiteralPath (Join-Path $repo 'Cargo.lock') -Algorithm SHA256).Hash;runtime_inventory_sha256=(Get-FileHash -LiteralPath $RuntimeManifest -Algorithm SHA256).Hash;baseline_manifest_sha256=$null;source_files=$sourceFiles;source_snapshot_scope='Packaging-time source snapshot; does not by itself attest which sources produced a reused executable.';toolchain=$toolchain}
    if ($BaselineManifest) { $provenance.baseline_manifest_sha256=(Get-FileHash -LiteralPath $BaselineManifest -Algorithm SHA256).Hash }
    $out=@{schema_version=1;version=$version;distribution_status=$m.distribution_status;components=$publicComponents;static_components=$m.static_components;files=$files;pe_imports=$closure;dynamic_dependencies=@('pdfium.dll');caveat='PE closure cannot prove statically linked third-party contents or arbitrary runtime LoadLibrary names; component review and real smoke remain mandatory.';build=@{native=$true;offline=$true;locked=$true;reused_build=[bool]$SkipBuild};smoke='NOT RUN'}
    $out.provenance=$provenance
    $out | ConvertTo-Json -Depth 30 | Set-Content -LiteralPath (Join-Path $dest 'release-manifest.json') -Encoding utf8
    & (Join-Path $PSScriptRoot 'smoke-windows-portable.ps1') -PackageRoot $dest -BaselineManifest $BaselineManifest -BaselineRoot $BaselineRoot -PythonPath $PythonPath
    $out.smoke=if ($BaselineRoot) { 'PASS: controlled PATH, accepted semantic baseline; clean Windows evidence is separate' } else { 'EXECUTION PASS; BASELINE NOT RUN; NOT RELEASE ACCEPTANCE' }
    $out | ConvertTo-Json -Depth 30 | Set-Content -LiteralPath (Join-Path $dest 'release-manifest.json') -Encoding utf8
    if ($CreateZip) {
        $zip="$dest.zip"; if (Test-Path -LiteralPath $zip) { throw 'ZIP exists' }
        Compress-Archive -LiteralPath $dest -DestinationPath $zip
        $extract=Join-Path $approved ('zip-smoke-'+[guid]::NewGuid().ToString('N'))
        Expand-Archive -LiteralPath $zip -DestinationPath $extract
        & (Join-Path $PSScriptRoot 'smoke-windows-portable.ps1') -PackageRoot (Join-Path $extract (Split-Path $dest -Leaf)) -BaselineManifest $BaselineManifest -BaselineRoot $BaselineRoot -PythonPath $PythonPath
        Get-FileHash -LiteralPath $zip -Algorithm SHA256
    }
} finally { Pop-Location }
