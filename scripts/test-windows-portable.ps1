# Source-only packaging checks: no OCR runtime, Cargo, private data or network.
$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
$repo=Split-Path $PSScriptRoot -Parent
foreach ($file in @(Get-ChildItem -LiteralPath $PSScriptRoot -Filter '*portable*.ps1')+@(Get-Item (Join-Path $PSScriptRoot 'portable-pe.ps1'))+@(Get-ChildItem (Join-Path $repo 'deploy/windows-portable') -Filter '*.ps1')) {
    $tokens=$null; $errors=$null
    [Management.Automation.Language.Parser]::ParseFile($file.FullName,[ref]$tokens,[ref]$errors) | Out-Null
    if ($errors.Count) { throw "Parse failure: $($file.Name): $errors" }
}
. (Join-Path $PSScriptRoot 'portable-pe.ps1')
function Expect-Rejection([scriptblock]$Action,[string]$Label) {
    $rejected=$false
    try { & $Action | Out-Null } catch { $rejected=$true }
    if (!$rejected) { throw "Expected rejection: $Label" }
    Write-Output "PASS: $Label"
}
$imports=@(Get-PortablePeImports (Join-Path $env:SystemRoot 'System32/notepad.exe'))
if ($imports.Count -lt 1 -or @($imports | Where-Object { $_ -match '^(api-ms-win-|kernel32\.|user32\.)' }).Count -lt 1) { throw 'Real PE import inspection failed' }
Write-Output 'PASS: real Windows x64 imports'
$work=Join-Path ([IO.Path]::GetTempPath()) ('ocr-portable-source-tests-'+[guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $work | Out-Null
try {
    $truncated=Join-Path $work 'truncated.exe'; [IO.File]::WriteAllBytes($truncated,[byte[]]@(77,90))
    Expect-Rejection { Get-PortablePeImports $truncated } 'truncated PE'
    $x86=Join-Path $env:SystemRoot 'SysWOW64/notepad.exe'
    if (Test-Path -LiteralPath $x86) { Expect-Rejection { Get-PortablePeImports $x86 } 'x86 PE' }
    $blocked=Join-Path $work 'blocked.json'
    @{schema_version=1;distribution_status='blocked';components=@(@{id='test';version='1';source_url='https://example.invalid/pinned';license='UNKNOWN';distribution_status='blocked';license_review='not reviewed';notices=@('licenses/test.txt');files=@()});static_components=@(@{id='static-test';version='1';source_url='https://example.invalid/pinned';license='UNKNOWN';distribution_status='blocked';license_review='not reviewed';notices=@('licenses/test.txt');sha256=('0'*64)})} | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $blocked
    $dest=Join-Path $work 'must-not-exist'
    Expect-Rejection { & (Join-Path $PSScriptRoot 'build-windows-portable.ps1') -RuntimeManifest $blocked -Destination $dest -ApprovedOutputRoot $work -CreateZip } 'blocked rights before payload/build'
    if ((Test-Path -LiteralPath $dest) -or (Test-Path -LiteralPath "$dest.zip")) { throw 'Blocked licensing created payload or ZIP' }
    $payload=Join-Path $work 'payload'; New-Item -ItemType Directory -Path (Join-Path $payload 'tessdata') -Force | Out-Null
    $files=@()
    foreach ($name in @('ocr-pipeline.exe','pdfium.dll','tessdata/eng.traineddata','tessdata/rus.traineddata')) {
        $path=Join-Path $payload $name; [IO.File]::WriteAllText($path,'synthetic test bytes; never executable')
        $files+=@{path=$name;sha256=(Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash}
    }
    $manifest=Join-Path $payload 'release-manifest.json'
    @{schema_version=1;files=$files} | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $manifest
    & (Join-Path $repo 'deploy/windows-portable/verify.ps1') -Root $payload
    [IO.File]::AppendAllText((Join-Path $payload 'pdfium.dll'),'changed')
    Expect-Rejection { & (Join-Path $repo 'deploy/windows-portable/verify.ps1') -Root $payload } 'changed payload hash'
    @{schema_version=1;files=@(@{path='../escape';sha256=('0'*64)})} | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $manifest
    Expect-Rejection { & (Join-Path $repo 'deploy/windows-portable/verify.ps1') -Root $payload } 'manifest path traversal'
    Write-Output 'Portable source-only checks PASS (not native OCR acceptance)'
} finally {
    # Deliberately retain owned synthetic test evidence; no recursive deletion.
    Write-Output "Retained synthetic test evidence: $work"
}
