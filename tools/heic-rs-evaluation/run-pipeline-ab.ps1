[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$BaselineExe,
    [Parameter(Mandatory)][string]$CandidateExe,
    [Parameter(Mandatory)][string]$Tessdata,
    [Parameter(Mandatory)][string]$Pdfium,
    [Parameter(Mandatory)][string]$OutputRoot,
    [ValidateSet('release', 'debug', 'not-recorded')][string]$BuildProfile = 'not-recorded',
    [int]$TimeoutSeconds = 120
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ($TimeoutSeconds -lt 1 -or $TimeoutSeconds -gt 600) { throw 'Invalid timeout' }
$project = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
foreach ($path in @($BaselineExe, $CandidateExe, $Tessdata, $Pdfium)) {
    if (![IO.Path]::IsPathFullyQualified($path) -or !(Test-Path -LiteralPath $path)) {
        throw 'All binary and runtime inputs must be existing absolute paths'
    }
}
if (Test-Path -LiteralPath $OutputRoot) { throw 'Refuse to reuse an output root' }
$root = (New-Item -ItemType Directory -Path $OutputRoot).FullName
$cases = @(
    @{ id = 'form'; file = 'tools/heic-rs-evaluation/fixtures/ocr-public/form-fixture.heic'; lane = 'text' },
    @{ id = 'note'; file = 'tools/heic-rs-evaluation/fixtures/ocr-public/note-fixture.heic'; lane = 'text' },
    @{ id = 'receipt'; file = 'tools/heic-rs-evaluation/fixtures/ocr-public/receipt-fixture.heic'; lane = 'text' },
    @{ id = 'text-table'; file = 'examples/documents/synthetic-text-table.pdf'; lane = 'pdf_control' },
    @{ id = 'raster'; file = 'examples/documents/synthetic-raster.pdf'; lane = 'pdf_control' },
    @{ id = 'main10'; file = 'tools/heic-rs-evaluation/fixtures/extended/gray-main10-64.heic'; lane = 'format_only' },
    @{ id = 'alpha'; file = 'tools/heic-rs-evaluation/fixtures/extended/flat-alpha128-64.heic'; lane = 'format_only' },
    @{ id = 'irot90'; file = 'tools/heic-rs-evaluation/fixtures/extended/strips-irot90.heic'; lane = 'format_only' },
    @{ id = 'imir0'; file = 'tools/heic-rs-evaluation/fixtures/extended/gradient-imir0.heic'; lane = 'format_only' },
    @{ id = 'grid'; file = 'tools/heic-rs-evaluation/fixtures/checker-1024.heic'; lane = 'format_only' },
    @{ id = 'white16'; file = 'tools/heic-rs-evaluation/fixtures/flat-white-16.heic'; lane = 'format_only' }
)
$runs = [Collections.Generic.List[object]]::new()
$variants = @{ native = $BaselineExe; heic_rs = $CandidateExe }
foreach ($variant in @('native', 'heic_rs')) {
    New-Item -ItemType Directory -Path (Join-Path $root $variant) | Out-Null
}
$caseIndex = 0
foreach ($case in $cases) {
    $inputPath = Join-Path $project $case.file
    if (!(Test-Path -LiteralPath $inputPath)) { throw "Missing fixture: $($case.id)" }
    # Alternate order to reduce systematic warm-cache bias; timings are not a benchmark.
    $order = if (($caseIndex++ % 2) -eq 0) { @('native', 'heic_rs') } else { @('heic_rs', 'native') }
    foreach ($variant in $order) {
        $parent = Join-Path $root $variant
        $output = Join-Path $parent $case.id
        $logPrefix = Join-Path $parent $case.id
        $start = [Diagnostics.ProcessStartInfo]::new()
        $start.FileName = $variants[$variant]
        $start.WorkingDirectory = $project
        $start.UseShellExecute = $false
        $start.CreateNoWindow = $true
        $start.RedirectStandardInput = $true
        $start.RedirectStandardOutput = $true
        $start.RedirectStandardError = $true
        $start.Environment['OMP_THREAD_LIMIT'] = '1'
        $arguments = @('--input', $inputPath, '--output-dir', $output,
            '--languages', 'rus+eng', '--tessdata-path', $Tessdata,
            '--pdfium-dll', $Pdfium, '--ocr-dpi', '300', '--grid-dpi', '100',
            '--max-page-workers', '4', '--enable-cell-ocr-fallback', 'true',
            '--progress-jsonl', ($logPrefix + '.progress.jsonl'))
        foreach ($argument in $arguments) { $start.ArgumentList.Add($argument) }
        $process = [Diagnostics.Process]::new()
        $process.StartInfo = $start
        $watch = [Diagnostics.Stopwatch]::StartNew()
        try {
            if (!$process.Start()) { throw 'Owned OCR process failed to start' }
            $process.StandardInput.Close()
            $stdout = $process.StandardOutput.ReadToEndAsync()
            $stderr = $process.StandardError.ReadToEndAsync()
            $timedOut = !$process.WaitForExit($TimeoutSeconds * 1000)
            if ($timedOut) {
                # Only the handle-owned child and descendants, never a process name.
                $process.Kill($true)
                if (!$process.WaitForExit(10000)) { throw 'Owned process did not terminate' }
            }
            $watch.Stop()
            # Bound pipe draining too: an inherited handle must not silently
            # turn a completed child into an unbounded diagnostic runner.
            $drains = [Threading.Tasks.Task]::WhenAll([Threading.Tasks.Task[]]@($stdout, $stderr))
            $captureIncomplete = !$drains.Wait(10000)
            $outText = if ($stdout.IsCompletedSuccessfully) { $stdout.Result } else { '[stdout capture incomplete]' }
            $errText = if ($stderr.IsCompletedSuccessfully) { $stderr.Result } else { '[stderr capture incomplete]' }
            [IO.File]::WriteAllText($logPrefix + '.stdout.log', $outText)
            [IO.File]::WriteAllText($logPrefix + '.stderr.log', $errText)
            $artifacts = @()
            foreach ($name in @('result.v1.json', 'result.json', 'result.docx', 'result.xlsx')) {
                $path = Join-Path $output $name
                if (Test-Path -LiteralPath $path) {
                    $file = Get-Item -LiteralPath $path
                    $artifacts += @{ name = $name; size = $file.Length; sha256 = (Get-FileHash -LiteralPath $path).Hash.ToLowerInvariant() }
                }
            }
            $runs.Add(@{ case = $case.id; lane = $case.lane; variant = $variant;
                input = $case.file; input_sha256 = (Get-FileHash -LiteralPath $inputPath).Hash.ToLowerInvariant();
                exit_code = $process.ExitCode; timeout = $timedOut; elapsed_ms = $watch.ElapsedMilliseconds;
                capture_incomplete = $captureIncomplete;
                output = "$variant/$($case.id)"; artifacts = $artifacts })
            Write-Output ("{0}/{1}: exit={2}, timeout={3}, ms={4}, artifacts={5}" -f
                $case.id, $variant, $process.ExitCode, $timedOut, $watch.ElapsedMilliseconds, $artifacts.Count)
        } finally { $process.Dispose() }
    }
}
$receipt = @{ schema_version = 1; scope = 'Research-only full pipeline A/B, not distribution or clean-Windows qualification';
    build_profile = $BuildProfile;
    settings = @{ languages = 'rus+eng'; ocr_dpi = 300; grid_dpi = 100; max_page_workers = 4;
        enable_cell_ocr_fallback = $true; omp_thread_limit = 1; timeout_seconds = $TimeoutSeconds };
    binaries = @($variants.Keys | Sort-Object | ForEach-Object {
        @{ variant = $_; size = (Get-Item -LiteralPath $variants[$_]).Length;
            sha256 = (Get-FileHash -LiteralPath $variants[$_]).Hash.ToLowerInvariant() }
    }); runs = @($runs.ToArray()) }
[IO.File]::WriteAllText((Join-Path $root 'runs.json'), ($receipt | ConvertTo-Json -Depth 10))
