[CmdletBinding()]
param([Parameter(Mandatory)][string]$PackageRoot,[string]$BaselineManifest,[string]$BaselineRoot,[string]$PythonPath)
$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
$package=[IO.Path]::GetFullPath($PackageRoot)
& (Join-Path $package 'verify.ps1') -Root $package
$workspace=Join-Path ([IO.Path]::GetTempPath()) ('OCR проверка с пробелами '+[guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $workspace | Out-Null
$copy=Join-Path $workspace 'Программа OCR'
Copy-Item -LiteralPath $package -Destination $copy -Recurse
$oldPath=$env:PATH; $oldTess=$env:TESSDATA_PREFIX; $oldPdf=$env:PDFIUM_DYNAMIC_LIB_PATH
$results=@(); $negativeResults=@()
if ($BaselineRoot -and (!$PythonPath -or ![IO.Path]::IsPathRooted($PythonPath))) { throw 'Semantic baseline comparison requires absolute maintainer PythonPath (not a user runtime dependency)' }
if ($BaselineManifest -and !$BaselineRoot) { throw 'BaselineManifest alone is not semantic comparison; supply BaselineRoot' }
$candidateRoot=Join-Path $workspace 'candidate'
New-Item -ItemType Directory -Path $candidateRoot | Out-Null
function Invoke-BoundedPortable([string[]]$Arguments,[string]$Log) {
    $start=[Diagnostics.ProcessStartInfo]::new()
    $start.FileName=Join-Path $copy 'ocr-pipeline.exe'
    $start.WorkingDirectory=$workspace
    $start.UseShellExecute=$false; $start.CreateNoWindow=$true
    $start.RedirectStandardOutput=$true; $start.RedirectStandardError=$true; $start.RedirectStandardInput=$true
    foreach ($argument in $Arguments) { $start.ArgumentList.Add($argument) }
    $process=[Diagnostics.Process]::new(); $process.StartInfo=$start
    try {
        if (!$process.Start()) { throw 'Portable process failed to start' }
        $process.StandardInput.Close()
        $stdout=$process.StandardOutput.ReadToEndAsync(); $stderr=$process.StandardError.ReadToEndAsync()
        if (!$process.WaitForExit(120000)) {
            # This Process object owns exactly the child started above; never kill by name.
            $process.Kill($true); $process.WaitForExit(10000) | Out-Null
            $outText=if ($stdout.IsCompletedSuccessfully) { $stdout.Result } else { '[stdout incomplete after timeout]' }
            $errText=if ($stderr.IsCompletedSuccessfully) { $stderr.Result } else { '[stderr incomplete after timeout]' }
            [IO.File]::WriteAllText($Log,"timeout=true`n$outText`n$errText")
            throw "Portable process timeout after 120s: $Log"
        }
        $text=$stdout.GetAwaiter().GetResult()+[Environment]::NewLine+$stderr.GetAwaiter().GetResult()
        [IO.File]::WriteAllText($Log,$text)
        return @{exit_code=$process.ExitCode;text=$text}
    } finally { $process.Dispose() }
}
function Invoke-Ocr([string]$Fixture,[string]$Output,[switch]$ExpectFailure,[string[]]$ExtraArguments=@()) {
    $log=Join-Path $workspace ((Split-Path $Output -Leaf)+'.log')
    $arguments=@('--input',(Join-Path $copy "examples/$Fixture.pdf"),'--output-dir',$Output)+$ExtraArguments
    $run=Invoke-BoundedPortable $arguments $log
    $code=$run.exit_code
    if ($ExpectFailure) {
        if ($code -eq 0 -or (Test-Path -LiteralPath $Output)) { throw 'Missing-runtime negative test unexpectedly succeeded or published output' }
        return $run.text
    }
    if ($code -ne 0) { throw "OCR failed ($code), see $log" }
}
try {
    # Absolute executable path + unrelated CWD; PATH contains no development tools.
    $env:PATH="$env:SystemRoot\System32;$env:SystemRoot"
    $env:TESSDATA_PREFIX=$null; $env:PDFIUM_DYNAMIC_LIB_PATH=$null
    Push-Location $workspace
    try {
        $version=Invoke-BoundedPortable @('--version') (Join-Path $workspace 'version.log')
        if ($version.exit_code) { throw 'Executable startup failed' }
        foreach ($fixture in @('synthetic-text-table','synthetic-raster')) {
            $fixtureKey=if ($fixture -eq 'synthetic-text-table') { 'text-table' } else { 'raster' }
            $output=Join-Path $candidateRoot $fixtureKey
            Invoke-Ocr $fixture $output
            $v1=Get-Content -LiteralPath (Join-Path $output 'result.v1.json') -Raw | ConvertFrom-Json
            $expectedMode=if ($fixtureKey -eq 'text-table') { 'native_text+grid' } else { 'ocr+grid' }
            if ($expectedMode -notin @($v1.page_processing.processing_mode)) { throw "Expected processing route not exercised: $expectedMode" }
            $tableCount=0; $words=0
            foreach ($page in $v1.document.pages) {
                $tableCount+=@($page.tables).Count
                $text=$page.text
                foreach ($table in $page.tables) { foreach ($row in $table.rows) { foreach ($cell in $row) { $text+=' '+$cell.text } } }
                $words+=@([regex]::Matches($text,'\S+')).Count
            }
            if ($tableCount -lt 1 -or $words -lt 1) { throw 'Synthetic fixture must yield nonempty words and at least one table' }
            if ($fixtureKey -eq 'raster' -and @($v1.page_processing | Where-Object { $_.PSObject.Properties.Name -contains 'word_assignment' -and $_.word_assignment.recognized_words -gt 0 }).Count -eq 0) { throw 'Raster fixture did not exercise actual OCR words' }
            $checks=@()
            foreach ($name in @('result.json','result.v1.json','result.docx','result.xlsx')) {
                $path=Join-Path $output $name
                if (!(Test-Path -LiteralPath $path) -or (Get-Item -LiteralPath $path).Length -eq 0) { throw "Missing/empty output $name" }
                if ($name.EndsWith('.json')) { Get-Content -LiteralPath $path -Raw | ConvertFrom-Json | Out-Null }
                $hash=(Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash
                $checks+=@{path=$name;sha256=$hash}
            }
            $results+=@{name=$fixture;files=$checks}
        }
        foreach ($missing in @('pdfium.dll','tessdata/eng.traineddata','tessdata/rus.traineddata')) {
            $original=Join-Path $copy $missing; $held="$original.smoke-held"
            Move-Item -LiteralPath $original -Destination $held
            try {
                $diagnostic=Invoke-Ocr 'synthetic-raster' (Join-Path $workspace ('missing-'+[guid]::NewGuid().ToString('N'))) -ExpectFailure
                $reason=if ($missing -eq 'pdfium.dll') { '(?i)pdfium' } else { '(?i)(traineddata|tessdata|tesseract|language)' }
                if ($diagnostic -notmatch $reason) { throw "Negative test failed for unrelated reason: $missing" }
                $negativeResults+=@{case="missing:$missing";status='PASS';output_published=$false}
            }
            finally { Move-Item -LiteralPath $held -Destination $original }
        }
        # Adjacent valid models remain untouched: invalid explicit overrides must not silently fall back.
        $invalidTess=Join-Path $workspace 'definitely absent tessdata'
        $diagnostic=Invoke-Ocr 'synthetic-raster' (Join-Path $workspace 'invalid-explicit-tessdata') -ExpectFailure -ExtraArguments @('--tessdata-path',$invalidTess)
        if ($diagnostic -notmatch '(?i)(tessdata|traineddata)' -or $diagnostic -notmatch '(?i)(explicit|--tessdata-path|does not exist|not found|directory)') { throw 'Invalid explicit tessdata path did not produce a relevant diagnostic' }
        $negativeResults+=@{case='invalid-explicit-tessdata-path';status='PASS';valid_adjacent_models_present=$true;output_published=$false}
        $savedPrefix=$env:TESSDATA_PREFIX
        try {
            $env:TESSDATA_PREFIX=$invalidTess
            $diagnostic=Invoke-Ocr 'synthetic-raster' (Join-Path $workspace 'invalid-tessdata-prefix') -ExpectFailure
            if ($diagnostic -notmatch '(?i)(TESSDATA_PREFIX|tessdata|traineddata)' -or $diagnostic -notmatch '(?i)(does not exist|not found|directory|invalid)') { throw 'Invalid TESSDATA_PREFIX did not produce a relevant diagnostic' }
            $negativeResults+=@{case='invalid-TESSDATA_PREFIX';status='PASS';valid_adjacent_models_present=$true;output_published=$false}
        } finally { $env:TESSDATA_PREFIX=$savedPrefix }
    } finally { Pop-Location }
    $status='EXECUTION PASS; BASELINE NOT RUN'
    if ($BaselineRoot) {
        & $PythonPath (Join-Path $PSScriptRoot 'compare_audit_results.py') --baseline $BaselineRoot --candidate $candidateRoot --report-dir (Join-Path $workspace 'comparison')
        if ($LASTEXITCODE) { throw 'Semantic OCR/geometry/API/export baseline regression' }
        $status='PASS'
    }
    @{status=$status;environment='controlled process-local PATH; not a clean Windows VM';workspace=$workspace;fixtures=$results;negative_tests='PASS';negative_test_details=$negativeResults;semantic_ocr_table_gate='requires accepted baseline; output existence alone is not OCR/table acceptance'} | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath (Join-Path $workspace 'smoke-report.json') -Encoding utf8
    Write-Output "Smoke $status. Retained evidence: $workspace"
} finally { $env:PATH=$oldPath; $env:TESSDATA_PREFIX=$oldTess; $env:PDFIUM_DYNAMIC_LIB_PATH=$oldPdf }
