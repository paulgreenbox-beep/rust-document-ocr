# Synthetic profile checks only: no Cargo, OCR, license overrides or clean-OS claims.
param([string]$WorkRoot=[IO.Path]::GetTempPath())
$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot 'portable-build-profile.ps1')
$work=Join-Path $WorkRoot ('portable-profile-tests-'+[guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $work | Out-Null
$repo=Join-Path $work 'source'
New-Item -ItemType Directory -Path (Join-Path $repo 'src') | Out-Null
function Write-Json($Path,$Object) { $Object | ConvertTo-Json -Depth 40 | Set-Content -LiteralPath $Path -Encoding utf8 }
function Hash($Path) { return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash }
function Check([string]$Label,[scriptblock]$Action,[string]$ErrorPattern='') {
    try { & $Action | Out-Null; if ($ErrorPattern) { throw "Expected rejection: $Label" } }
    catch { if (!$ErrorPattern -or $_.Exception.Message -notlike "*$ErrorPattern*") { throw } }
    Write-Output "PASS: $Label"
}
try {
    [IO.File]::WriteAllText((Join-Path $repo 'Cargo.lock'),'synthetic lock')
    [IO.File]::WriteAllText((Join-Path $repo 'Cargo.toml'),'synthetic manifest')
    [IO.File]::WriteAllText((Join-Path $repo 'src/main.rs'),'synthetic Rust; never compiled')
    & git -c core.longpaths=true -C $repo init -q
    if ($LASTEXITCODE) { throw 'Synthetic Git init failed' }
    & git -C $repo config core.longpaths true
    if ($LASTEXITCODE) { throw 'Synthetic Git long-path setup failed' }
    & git -C $repo add .
    if ($LASTEXITCODE) { throw 'Synthetic Git add failed' }
    & git -C $repo -c user.name=ProfileFixture -c user.email=fixture@example.invalid commit -qm synthetic
    if ($LASTEXITCODE) { throw 'Synthetic Git setup failed' }
    $commit=(& git -C $repo rev-parse HEAD).Trim()
    $tree=(& git -C $repo rev-parse 'HEAD^{tree}').Trim()
    $exe=Join-Path $work 'synthetic-not-executable.exe'
    $model=Join-Path $work 'synthetic-model'
    [IO.File]::WriteAllText($exe,'synthetic EXE bytes; never run')
    [IO.File]::WriteAllText($model,'synthetic model bytes; never used')
    $manifestPath=Join-Path $work 'runtime.json'
    $originalPath=Join-Path $work 'original.json'
    $linkPath=Join-Path $work 'link.json'
    $mapPath=Join-Path $work 'synthetic.map'
    [IO.File]::WriteAllText($mapPath,'synthetic map; never linker evidence')
    $receiptPath=Join-Path $work 'authorization.json'
    $manifest=@{schema_version=1;components=@(@{id='technical-cell-model-eg79';files=@(@{path='tessdata/eng_technical_eg79_v1.traineddata';source=$model;sha256=(Hash $model)})})}
    $original=@{source_sha=$commit;git_tree_sha=$tree;cargo_lock_sha256=(Hash (Join-Path $repo 'Cargo.lock'));sha256=(Hash $exe);target='x86_64-pc-windows-msvc';profile='release';offline=$true;locked=$true;exit_code=0;feature_set=@('heic-rs-input');command=@('cargo','rustc','--release','--offline','--locked','--no-default-features','--features','heic-rs-input','--bin','ocr-pipeline');map_sha256=('a'*64)}
    $link=@{schema_version=1;exe_sha256=(Hash $exe);map_sha256=('a'*64);link_exit=0;explicit_old_inputs=@();map_old_hits=@();verbose_old_hits=@();pe_old_import_match=$false;old_dll_literal_matches=@{};response_files=@(@{sha256=('b'*64)});resolved_inputs=@(@{name='libheic_rs-synthetic.rlib';sha256=('c'*64)});archive_scans=@(@{archive='libheic_rs-synthetic.rlib';sha256=('c'*64);exit_code=0;old_heif_symbols=@()})}
    foreach ($dll in @('heif.dll','libheif.dll','de265.dll','libde265.dll')) { $link.old_dll_literal_matches[$dll]=@{ascii=$false;utf16le=$false} }
    $receipt=@{schema_version=1;receipt_kind='portable-pure-heic-authorization';build_profile='pure-heic';source_commit=$commit;git_tree=$tree;cargo_lock_sha256=$original.cargo_lock_sha256;features=@('heic-rs-input');no_default_features=$true;target=$original.target;profile='release';offline=$true;locked=$true;exe_path=$exe;exe_sha256=(Hash $exe);runtime_manifest_sha256='';original_build_receipt=@{path=$originalPath;sha256=''};link_evidence=@{path=$linkPath;sha256=''}}
    $original.map_sha256=Hash $mapPath
    $link.map_sha256=Hash $mapPath
    $link.archive_scans[0].symbol_lines=0
    $receipt.link_map=@{path=$mapPath;sha256=(Hash $mapPath)}
    function Save-All {
        Write-Json $manifestPath $manifest
        Write-Json $originalPath $original
        Write-Json $linkPath $link
        $receipt.runtime_manifest_sha256=Hash $manifestPath
        $receipt.original_build_receipt.sha256=Hash $originalPath
        $receipt.link_evidence.sha256=Hash $linkPath
        Write-Json $receiptPath $receipt
    }
    function Validate { Get-PortablePureAuthorization -Repo $repo -ReceiptPath $receiptPath -ExpectedReceiptSha256 (Hash $receiptPath) -RuntimeManifest $manifestPath -VerifyExecutable }
    Save-All
    Check 'valid synthetic binding (not real build acceptance)' { Validate }
    Check 'JSON snapshot rejects mismatching expected hash' { Read-PortableStrictJson $receiptPath ('0'*64) 'synthetic JSON snapshot' } 'Hash mismatch: synthetic JSON snapshot'
    Check 'pure exact Cargo arguments' {
        $argsList=@(Get-PortableProfileCargoArguments 'pure-heic')
        if (($argsList -join ' ') -cne 'build --release --offline --locked --no-default-features --features heic-rs-input --target x86_64-pc-windows-msvc --bin ocr-pipeline -j 1') { throw 'Unexpected pure Cargo command' }
    }
    Check 'legacy compatibility arguments' { if ((@(Get-PortableProfileCargoArguments 'legacy-native') -join ' ') -cne 'build --release --offline --locked --features native --bin ocr-pipeline -j 1') { throw 'Legacy changed' } }
    foreach ($case in @(
        @('native feature','features',@('native'),'feature/target/profile'),
        @('mixed features','features',@('heic-rs-input','native'),'feature/target/profile'),
        @('wrong target','target','x86_64-unknown-linux-gnu','feature/target/profile'),
        @('wrong profile','profile','debug','feature/target/profile'),
        @('defaults enabled','no_default_features',$false,'feature/target/profile'),
        @('offline missing','offline',$false,'feature/target/profile'),
        @('locked missing','locked',$false,'feature/target/profile'),
        @('string boolean','offline','true','feature/target/profile'),
        @('wrong schema','schema_version',2,'schema/profile'),
        @('string schema','schema_version','1','schema/profile'),
        @('legacy receipt label','receipt_kind','native-build','schema/profile'),
        @('wrong EXE hash','exe_sha256',('d'*64),'exact pure inputs'),
        @('wrong source','source_commit',('0'*40),'existing Git commit'),
        @('wrong tree','git_tree',('0'*40),'tree does not match'),
        @('wrong lock','cargo_lock_sha256',('0'*64),'Cargo.lock')
    )) {
        $old=$receipt[$case[1]]; $receipt[$case[1]]=$case[2]; Save-All
        Check $case[0] { Validate } $case[3]
        $receipt[$case[1]]=$old
    }
    Save-All
    Check 'missing trusted receipt hash' { Get-PortablePureAuthorization -Repo $repo -ReceiptPath $receiptPath -RuntimeManifest $manifestPath } 'trusted ExpectedBuildReceiptSha256'
    $pinned=Hash $receiptPath
    [IO.File]::AppendAllText($receiptPath,' ')
    Check 'tampered authorization bytes' { Get-PortablePureAuthorization -Repo $repo -ReceiptPath $receiptPath -ExpectedReceiptSha256 $pinned -RuntimeManifest $manifestPath } 'pure authorization'
    Save-All
    [IO.File]::AppendAllText($linkPath,' ')
    Check 'tampered link artifact' { Validate } 'link evidence artifact'
    Save-All
    [IO.File]::AppendAllText($originalPath,' ')
    Check 'tampered original receipt' { Validate } 'original build receipt'
    Save-All
    [IO.File]::AppendAllText($manifestPath,' ')
    Check 'wrong runtime manifest identity' { Validate } 'runtime manifest identity'
    Save-All
    [IO.File]::AppendAllText($exe,'changed')
    Check 'wrong executable bytes' { Validate } 'pure executable'
    [IO.File]::WriteAllText($exe,'synthetic EXE bytes; never run')
    $manifest.components=@(); Save-All
    Check 'missing EG79' { Validate } 'EG79 component'
    $manifest.components=@(@{id='technical-cell-model-eg79';files=@(@{path='tessdata/eng.traineddata';source=$model;sha256=(Hash $model)})}); Save-All
    Check 'EG79 substituted with English model path' { Validate } 'actual technical tessdata'
    $manifest.components[0].files[0].path='tessdata/eng_technical_eg79_v1.traineddata'
    $manifest.components[0].files[0].sha256='0'*64; Save-All
    Check 'EG79 wrong bytes' { Validate } 'EG79 model'
    $manifest.components[0].files[0].sha256=Hash $model
    $link.map_old_hits=@('heif_decode_image'); Save-All
    Check 'legacy linker symbol' { Validate } 'old HEIF exclusion'
    $link.map_old_hits=@(); $link.Remove('verbose_old_hits'); Save-All
    Check 'missing old HEIF proof' { Validate } 'old HEIF exclusion'
    $link.verbose_old_hits=@(); $link.archive_scans[0].old_heif_symbols=@('de265_new_decoder'); Save-All
    Check 'legacy archive symbol' { Validate } 'archive scan'
    $link.archive_scans[0].old_heif_symbols=@(); $link.archive_scans[0].sha256='0'*64; Save-All
    Check 'archive scan not bound to input' { Validate } 'hash-bound symbol scan'
    $link.archive_scans[0].sha256='c'*64
    $link.resolved_inputs[0].name='heif.lib'; Save-All
    Check 'exact heif.lib rejected' { Validate } 'Invalid/legacy link input'
    $link.resolved_inputs[0].name='libheic_rs-synthetic.rlib'
    foreach ($badLines in @(-1,'0',$null)) {
        $link.archive_scans[0].symbol_lines=$badLines; Save-All
        Check 'invalid archive symbol count' { Validate } 'archive scan'
    }
    $link.archive_scans[0].symbol_lines=0
    $original.feature_set=@('native'); Save-All
    Check 'native original cannot be relabeled pure' { Validate } 'exact pure inputs'
    $original.feature_set=@('heic-rs-input'); $original.command+=@('--all-features'); Save-All
    Check 'mixed original command' { Validate } 'Mixed/ambiguous Cargo'
    $original.command=$original.command[0..9]; Save-All
    foreach ($flags in @(@('-F','native'),@('-Fnative'),@('--no-default-features=false'))) {
        $oldCommand=$original.command
        $original.command+=@($flags); Save-All
        Check 'alternate/ambiguous Cargo feature flag' { Validate } 'Mixed/ambiguous Cargo'
        $original.command=$oldCommand
    }
    Save-All
    [IO.File]::AppendAllText($mapPath,'changed')
    Check 'tampered actual link map' { Validate } 'link map artifact'
    [IO.File]::WriteAllText($mapPath,'synthetic map; never linker evidence')
    $receipt.link_map.sha256='0'*64; Save-All
    Check 'wrong map binding' { Validate } 'link map artifact'
    $receipt.link_map.sha256=Hash $mapPath; Save-All
    [IO.File]::AppendAllText((Join-Path $repo 'src/main.rs'),'changed')
    Check 'modified Rust input' { Validate } 'Compiled input differs'
    [IO.File]::WriteAllText((Join-Path $repo 'src/main.rs'),'synthetic Rust; never compiled')
    [IO.File]::WriteAllText((Join-Path $repo 'new-build-input.rs'),'untracked')
    Check 'untracked build input' { Validate } 'Compiled input differs'
    # Retain fixture; move only this owned file to a packaging-only path.
    New-Item -ItemType Directory -Path (Join-Path $repo 'docs') | Out-Null
    Move-Item -LiteralPath (Join-Path $repo 'new-build-input.rs') -Destination (Join-Path $repo 'docs/fixture.txt')
    Check 'packaging-only changes allow original source' { Validate }
    # Exercise the production entry point without granting any rights or ever
    # invoking Cargo: profile authorization failures precede all output/build.
    $guardManifest=@{schema_version=1;distribution_status='blocked';components=@(@{id='technical-cell-model-eg79';version='synthetic';source_url='urn:synthetic';license='UNRESOLVED';distribution_status='blocked';license_review='synthetic test only';notices=@();files=@(@{path='tessdata/eng_technical_eg79_v1.traineddata';source=$model;sha256=(Hash $model)})});static_components=@(@{id='synthetic-static';version='1';source_url='urn:synthetic';license='UNRESOLVED';distribution_status='blocked';license_review='synthetic test only';notices=@();sha256=('0'*64)})}
    $guardPath=Join-Path $work 'guard-runtime.json'; Write-Json $guardPath $guardManifest
    $guardDest=Join-Path $work 'must-not-be-created'
    Check 'normal pure build rejects missing authorization before Cargo/output' {
        & (Join-Path $PSScriptRoot 'build-windows-portable.ps1') -RuntimeManifest $guardPath -Destination $guardDest -ApprovedOutputRoot $work -BuildProfile pure-heic
    } 'trusted ExpectedBuildReceiptSha256'
    $receipt.features=@('native'); Save-All
    Check 'reuse pure rejects native authorization before output' {
        & (Join-Path $PSScriptRoot 'build-windows-portable.ps1') -RuntimeManifest $guardPath -Destination $guardDest -ApprovedOutputRoot $work -BuildProfile pure-heic -SkipBuild -BuildReceipt $receiptPath -ExpectedBuildReceiptSha256 (Hash $receiptPath)
    } 'feature/target/profile'
    if (Test-Path -LiteralPath $guardDest) { throw 'Failed preflight created output' }
    foreach ($json in @('{"schema_version":1,"schema_version":2}','{"a":{"Key":1,"key":2}}','{"a":1,"\u0061":2}')) {
        [IO.File]::WriteAllText($receiptPath,$json)
        Check 'duplicate/ambiguous JSON keys' { Read-PortableStrictJson $receiptPath } 'Duplicate/ambiguous JSON key'
    }
    Write-Output 'Pure profile synthetic checks PASS; no actual build, runtime or clean Windows acceptance.'
} finally { Write-Output "Retained synthetic profile evidence: $work" }
