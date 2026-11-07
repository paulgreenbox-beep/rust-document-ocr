# Download only the reviewed allowlist from one immutable upstream commit.
[CmdletBinding()]
param()
$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
$receipt=Get-Content -LiteralPath (Join-Path $PSScriptRoot 'upstream-source.json') -Raw | ConvertFrom-Json
foreach ($record in $receipt.files) {
    $destination=Join-Path $PSScriptRoot $record.destination
    if (Test-Path -LiteralPath $destination) {
        $bytes=[IO.File]::ReadAllBytes($destination)
    } else {
        $url='https://raw.githubusercontent.com/tbraun96/heic-rs/'+$receipt.source_commit+'/'+$record.path
        $client=[Net.Http.HttpClient]::new()
        try { $bytes=$client.GetByteArrayAsync($url).GetAwaiter().GetResult() }
        finally { $client.Dispose() }
    }
    $header=[Text.Encoding]::ASCII.GetBytes('blob '+$bytes.Length+[char]0)
    $gitBytes=[byte[]]::new($header.Length+$bytes.Length)
    $header.CopyTo($gitBytes,0); $bytes.CopyTo($gitBytes,$header.Length)
    $hash=[Convert]::ToHexString([Security.Cryptography.SHA1]::HashData($gitBytes)).ToLowerInvariant()
    if ($bytes.Length -ne $record.size -or $hash -cne $record.git_blob_sha1) { throw "Upstream Git blob/size mismatch: $($record.path)" }
    if (!(Test-Path -LiteralPath $destination)) {
        [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($destination)) | Out-Null
        $file=[IO.File]::Open($destination,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::None)
        try { $file.Write($bytes) } finally { $file.Dispose() }
    }
    Write-Output ('VERIFIED '+$record.destination+' size='+$bytes.Length+' git_blob_sha1='+$hash+' sha256='+[Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($bytes)).ToLowerInvariant())
}
