# Pure receipts are owner-reviewed attestations, not signatures. The expected
# authorization hash must be obtained from a trusted channel, not from the file
# being checked. Never rewrite a legacy receipt to give it a pure profile label.
function Read-PortableStrictJson([string]$Path,[string]$ExpectedSha256,[string]$Label='JSON artifact') {
    if ($PSVersionTable.PSVersion.Major -lt 7) { throw 'Pure profile validation requires PowerShell 7+' }
    # Hash and parse one immutable byte snapshot, never reopen a checked JSON.
    $bytes=[IO.File]::ReadAllBytes([IO.Path]::GetFullPath($Path))
    if ($PSBoundParameters.ContainsKey('ExpectedSha256')) {
        $actual=[Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($bytes))
        if ($ExpectedSha256 -notmatch '^[a-fA-F0-9]{64}$' -or $actual -ne $ExpectedSha256) { throw "Hash mismatch: $Label" }
    }
    $raw=[Text.UTF8Encoding]::new($false,$true).GetString($bytes)
    if ($raw.Length -gt 0 -and $raw[0] -eq [char]0xFEFF) { $raw=$raw.Substring(1) }
    $doc=[Text.Json.JsonDocument]::Parse($raw)
    try {
        function Assert-JsonKeys($Element) {
            if ($Element.ValueKind -eq [Text.Json.JsonValueKind]::Object) {
                $names=[Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
                foreach ($p in $Element.EnumerateObject()) {
                    if (!$names.Add($p.Name)) { throw "Duplicate/ambiguous JSON key: $($p.Name)" }
                    Assert-JsonKeys $p.Value
                }
            } elseif ($Element.ValueKind -eq [Text.Json.JsonValueKind]::Array) {
                foreach ($item in $Element.EnumerateArray()) { Assert-JsonKeys $item }
            }
        }
        Assert-JsonKeys $doc.RootElement
        if ($doc.RootElement.ValueKind -ne [Text.Json.JsonValueKind]::Object) { throw 'JSON root must be an object' }
    } finally { $doc.Dispose() }
    return ($raw | ConvertFrom-Json -Depth 100)
}

function Assert-PortableHash([string]$Path,[string]$Expected,[string]$Label) {
    if ($Expected -notmatch '^[a-fA-F0-9]{64}$' -or (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash -ne $Expected) { throw "Hash mismatch: $Label" }
}

function Get-PortableProfileCargoArguments([string]$BuildProfile) {
    if ($BuildProfile -ceq 'pure-heic') {
        return @('build','--release','--offline','--locked','--no-default-features','--features','heic-rs-input','--target','x86_64-pc-windows-msvc','--bin','ocr-pipeline','-j','1')
    }
    if ($BuildProfile -ceq 'legacy-native') { return @('build','--release','--offline','--locked','--features','native','--bin','ocr-pipeline','-j','1') }
    throw 'Unsupported build profile'
}

function Test-PortablePackagingOnlyPath([string]$Path) {
    # Deliberately narrow exception list. All Rust/Cargo/native/vendor/assets,
    # unknown paths and configuration remain build inputs, even if unused today.
    # This one standalone integration-test file is not an input to --bin
    # ocr-pipeline. Do not expand this to all tests or arbitrary Rust paths.
    return ($Path -cmatch '(^[^/]+\.md$|^(docs|scripts|\.github|deploy/windows-portable)/|^tests/technical_document_capabilities\.rs$|^tools/heic-rs-evaluation/(README\.md|[^/]+\.(ps1|py)|pure-native-link-evidence\.json)$)')
}

function Assert-PortableSourceIdentity([string]$Repo,[string]$Commit,[string]$Tree,[string]$LockHash) {
    if ($Commit -cnotmatch '^[0-9a-f]{40}$' -or $Tree -cnotmatch '^[0-9a-f]{40}$') { throw 'Invalid source commit/tree identity' }
    $actual=(& git -C $Repo rev-parse --verify "$Commit^{commit}" 2>$null | Out-String).Trim()
    if ($LASTEXITCODE -or $actual -cne $Commit) { throw 'Source commit is not an existing Git commit' }
    $actual=(& git -C $Repo rev-parse --verify "$Commit^{tree}" 2>$null | Out-String).Trim()
    if ($LASTEXITCODE -or $actual -cne $Tree) { throw 'Source tree does not match source commit' }
    & git -C $Repo merge-base --is-ancestor $Commit HEAD
    if ($LASTEXITCODE) { throw 'Receipt source must be an ancestor of packaging HEAD' }
    $changes=@(& git -C $Repo -c core.quotePath=false diff --name-only $Commit --)
    if ($LASTEXITCODE) { throw 'Cannot compare compiled source inputs' }
    $untracked=@(& git -C $Repo -c core.quotePath=false ls-files --others --exclude-standard)
    if ($LASTEXITCODE) { throw 'Cannot inventory untracked inputs' }
    foreach ($p in @($changes)+@($untracked)) {
        if (!(Test-PortablePackagingOnlyPath $p)) { throw "Compiled input differs from receipt source: $p" }
    }
    Assert-PortableHash (Join-Path $Repo 'Cargo.lock') $LockHash 'Cargo.lock'
}

function Assert-PortablePureRuntime($Manifest) {
    $eg=@($Manifest.components | Where-Object { $_.id -ceq 'technical-cell-model-eg79' })
    if ($eg.Count -ne 1) { throw 'Pure profile requires exactly one retained EG79 component' }
    $files=@($eg[0].files | Where-Object { $_.path -ceq 'tessdata/eng_technical_eg79_v1.traineddata' })
    if ($files.Count -ne 1) { throw 'EG79 requires the actual technical tessdata payload path' }
    Assert-PortableHash $files[0].source $files[0].sha256 'EG79 model'
    # This preserves the model; it confers no redistribution permission.
}

function Assert-PortablePureStagedPayload([string]$PackageRoot,$Manifest,[string]$ExeHash) {
    Assert-PortableHash (Join-Path $PackageRoot 'ocr-pipeline.exe') $ExeHash 'staged pure executable'
    foreach ($component in $Manifest.components) { foreach ($file in $component.files) {
        Assert-PortableHash (Join-Path $PackageRoot $file.path) $file.sha256 'staged runtime payload'
    } }
}

function Assert-PortablePureLinkEvidence($Link,[string]$ExeHash,[string]$MapHash) {
    if ($Link.schema_version -isnot [long] -and $Link.schema_version -isnot [int] -or $Link.schema_version -ne 1 -or $Link.exe_sha256 -ne $ExeHash -or $Link.map_sha256 -ne $MapHash -or $Link.link_exit -isnot [long] -and $Link.link_exit -isnot [int] -or $Link.link_exit -ne 0) { throw 'Link evidence identity/exit mismatch' }
    foreach ($field in @('explicit_old_inputs','map_old_hits','verbose_old_hits')) {
        if (!($Link.PSObject.Properties.Name -ccontains $field) -or $Link.$field -isnot [array] -or @($Link.$field).Count) { throw "Missing/failed old HEIF exclusion proof: $field" }
    }
    if ($Link.pe_old_import_match -isnot [bool] -or $Link.pe_old_import_match) { throw 'Missing/failed old HEIF PE exclusion proof' }
    foreach ($dll in @('heif.dll','libheif.dll','de265.dll','libde265.dll')) {
        $literal=$Link.old_dll_literal_matches.PSObject.Properties[$dll]
        if (!$literal -or $literal.Value.ascii -isnot [bool] -or $literal.Value.ascii -or $literal.Value.utf16le -isnot [bool] -or $literal.Value.utf16le) { throw 'Missing/failed old HEIF DLL literal proof' }
    }
    if (@($Link.response_files).Count -lt 1 -or @($Link.resolved_inputs).Count -lt 1 -or @($Link.archive_scans).Count -lt 1) { throw 'Link evidence must include responses, resolved inputs and archive scans (not PE alone)' }
    foreach ($scan in $Link.archive_scans) {
        if ($scan.exit_code -isnot [long] -and $scan.exit_code -isnot [int] -or $scan.exit_code -ne 0 -or $scan.symbol_lines -isnot [long] -and $scan.symbol_lines -isnot [int] -or $scan.symbol_lines -lt 0 -or $scan.archive -isnot [string] -or !$scan.archive -or $scan.sha256 -notmatch '^[a-fA-F0-9]{64}$' -or $scan.old_heif_symbols -isnot [array] -or @($scan.old_heif_symbols).Count) { throw 'Failed old HEIF archive scan' }
    }
    foreach ($linkInput in $Link.resolved_inputs) {
        if ($linkInput.sha256 -notmatch '^[a-fA-F0-9]{64}$' -or $linkInput.name -match '(libheif|libde265|(^|[-_])(heif|de265)([-_.]|$))') { throw 'Invalid/legacy link input' }
        if ($linkInput.name -match '\.(rlib|lib)$') {
            $scans=@($Link.archive_scans | Where-Object { $_.archive -ceq $linkInput.name -and $_.sha256 -eq $linkInput.sha256 })
            if (!$scans.Count) { throw 'Resolved archive lacks hash-bound symbol scan' }
        }
    }
    if (@($Link.resolved_inputs | Where-Object { $_.name -cmatch '^libheic_rs-.*\.rlib$' }).Count -ne 1) { throw 'Pure heic-rs linker input missing/ambiguous' }
}

function Get-PortablePureAuthorization {
    param([string]$Repo,[string]$ReceiptPath,[string]$ExpectedReceiptSha256,[string]$RuntimeManifest,[switch]$VerifyExecutable)
    if (!$ReceiptPath -or !$ExpectedReceiptSha256) { throw 'Pure profile requires BuildReceipt and trusted ExpectedBuildReceiptSha256' }
    $r=Read-PortableStrictJson $ReceiptPath $ExpectedReceiptSha256 'pure authorization'
    if ($r.schema_version -isnot [long] -and $r.schema_version -isnot [int] -or $r.schema_version -ne 1 -or $r.receipt_kind -cne 'portable-pure-heic-authorization' -or $r.build_profile -cne 'pure-heic') { throw 'Unsupported pure authorization schema/profile' }
    if ($r.target -cne 'x86_64-pc-windows-msvc' -or $r.profile -cne 'release' -or $r.offline -isnot [bool] -or !$r.offline -or $r.locked -isnot [bool] -or !$r.locked -or $r.no_default_features -isnot [bool] -or !$r.no_default_features -or $r.features -isnot [array] -or @($r.features).Count -ne 1 -or $r.features[0] -cne 'heic-rs-input') { throw 'Invalid pure feature/target/profile contract' }
    Assert-PortableSourceIdentity $Repo $r.source_commit $r.git_tree $r.cargo_lock_sha256
    $runtime=Read-PortableStrictJson $RuntimeManifest $r.runtime_manifest_sha256 'runtime manifest identity'
    Assert-PortablePureRuntime $runtime
    $b=Read-PortableStrictJson $r.original_build_receipt.path $r.original_build_receipt.sha256 'original build receipt'
    if ($b.exit_code -isnot [long] -and $b.exit_code -isnot [int]) { throw 'Original build exit code must be an integer' }
    if ($b.source_sha -cne $r.source_commit -or $b.git_tree_sha -cne $r.git_tree -or $b.cargo_lock_sha256 -ne $r.cargo_lock_sha256 -or $b.sha256 -ne $r.exe_sha256 -or $b.target -cne $r.target -or $b.profile -cne $r.profile -or $b.offline -isnot [bool] -or !$b.offline -or $b.locked -isnot [bool] -or !$b.locked -or $b.exit_code -ne 0 -or $b.feature_set -isnot [array] -or @($b.feature_set).Count -ne 1 -or $b.feature_set[0] -cne 'heic-rs-input') { throw 'Original build receipt does not attest exact pure inputs' }
    $cmd=@($b.command)
    if ($cmd.Count -lt 3 -or $cmd[0] -cne 'cargo' -or $cmd[1] -cnotin @('build','rustc')) { throw 'Invalid original Cargo command' }
    foreach ($flag in @('--release','--offline','--locked','--no-default-features','--features','--bin')) {
        if (@($cmd | Where-Object { $_ -ceq $flag }).Count -ne 1) { throw "Missing/ambiguous Cargo flag: $flag" }
    }
    if ($cmd[[array]::IndexOf($cmd,'--features')+1] -cne 'heic-rs-input' -or $cmd[[array]::IndexOf($cmd,'--bin')+1] -cne 'ocr-pipeline' -or @($cmd | Where-Object { $_ -cmatch '^(-F|--(all-features|features=|no-default-features=|profile|target=))' }).Count) { throw 'Mixed/ambiguous Cargo features/profile' }
    if (@($cmd | Where-Object { $_ -ceq '--target' }).Count -gt 1) { throw 'Ambiguous Cargo target' }
    if ($cmd -ccontains '--target') { if ($cmd[[array]::IndexOf($cmd,'--target')+1] -cne $r.target) { throw 'Cargo target mismatch' } }
    $link=Read-PortableStrictJson $r.link_evidence.path $r.link_evidence.sha256 'link evidence artifact'
    Assert-PortablePureLinkEvidence $link $r.exe_sha256 $b.map_sha256
    Assert-PortableHash $r.link_map.path $r.link_map.sha256 'link map artifact'
    if ($r.link_map.sha256 -ne $b.map_sha256) { throw 'Link map does not match build/link evidence' }
    if (![IO.Path]::IsPathFullyQualified($r.exe_path)) { throw 'Authorized executable path must be absolute' }
    if ($r.exe_sha256 -notmatch '^[a-fA-F0-9]{64}$') { throw 'Invalid executable hash' }
    if ($VerifyExecutable) { Assert-PortableHash $r.exe_path $r.exe_sha256 'pure executable' }
    # Callers must package this exact hash-validated snapshot, not a separate
    # earlier read. An input cannot supply/override this reserved property.
    $r | Add-Member -NotePropertyName VerifiedRuntimeManifest -NotePropertyValue $runtime
    return $r
}
