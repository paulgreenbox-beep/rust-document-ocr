# Offline PE32+ import inspection; no developer tools or DLL execution.
Set-StrictMode -Version Latest
function Get-PortablePeImports([string]$Path) {
    $b = [IO.File]::ReadAllBytes($Path)
    if ($b.Length -lt 64) { throw "Truncated PE: $Path" }
    $pe = [BitConverter]::ToInt32($b,60)
    if ($pe -lt 0 -or $pe + 264 -gt $b.Length -or [BitConverter]::ToUInt32($b,$pe) -ne 0x4550 -or [BitConverter]::ToUInt16($b,$pe+4) -ne 0x8664) { throw "Not Windows x64 PE: $Path" }
    $opt = $pe+24
    if ([BitConverter]::ToUInt16($b,$opt) -ne 0x20b) { throw 'Expected PE32+' }
    $sections = @(); $count = [BitConverter]::ToUInt16($b,$pe+6)
    $sectionStart = $opt + [BitConverter]::ToUInt16($b,$pe+20)
    for ($i=0; $i -lt $count; $i++) {
        $s=$sectionStart+40*$i
        $sections += @{VA=[BitConverter]::ToUInt32($b,$s+12); Size=[Math]::Max([BitConverter]::ToUInt32($b,$s+8),[BitConverter]::ToUInt32($b,$s+16)); Raw=[BitConverter]::ToUInt32($b,$s+20)}
    }
    function Offset([long]$rva) {
        foreach ($s in $sections) { if ($rva -ge $s.VA -and $rva -lt ($s.VA+$s.Size)) { $o=$rva-$s.VA+$s.Raw; if ($o -ge $b.Length) { throw 'PE RVA outside file' }; return [int]$o } }
        throw "Unmapped PE RVA $rva"
    }
    $names = @()
    foreach ($spec in @(@{Index=1;Stride=20;Name=12},@{Index=13;Stride=32;Name=4})) {
        $rva=[BitConverter]::ToUInt32($b,$opt+112+8*$spec.Index)
        if (!$rva) { continue }; $o=Offset $rva
        while ($true) {
            if ($o+$spec.Stride -gt $b.Length) { throw 'Truncated import descriptor' }
            $nr=[BitConverter]::ToUInt32($b,$o+$spec.Name)
            if (!$nr) { break }
            if ($spec.Index -eq 13 -and ([BitConverter]::ToUInt32($b,$o) -band 1) -eq 0) { throw 'Legacy VA delay imports unsupported; fail closed' }
            $n=Offset $nr; $end=$n
            while ($end -lt $b.Length -and $b[$end] -ne 0) { $end++ }
            if ($end -eq $b.Length) { throw 'Unterminated import' }
            $names += [Text.Encoding]::ASCII.GetString($b,$n,$end-$n).ToLowerInvariant()
            $o += $spec.Stride
        }
    }
    return @($names | Sort-Object -Unique)
}
function Test-PortablePeClosure([string]$Root) {
    # VC runtime/UCRT are deliberately NOT wildcard-allowed: supply app-local redistributables.
    $system = @('kernel32.dll','ntdll.dll','user32.dll','gdi32.dll','advapi32.dll','shell32.dll','ole32.dll','oleaut32.dll','ws2_32.dll','bcrypt.dll','crypt32.dll','secur32.dll','shlwapi.dll','comdlg32.dll','comctl32.dll','version.dll','winmm.dll','imm32.dll','setupapi.dll','cfgmgr32.dll','rpcrt4.dll','userenv.dll','dbghelp.dll','dwmapi.dll','dwrite.dll','d2d1.dll','dxgi.dll','d3d11.dll','windowscodecs.dll','msvcrt.dll','ucrtbase.dll','normaliz.dll','psapi.dll','iphlpapi.dll','dnsapi.dll','netapi32.dll','wtsapi32.dll','winspool.drv','winhttp.dll','wininet.dll','propsys.dll','powrprof.dll','wintrust.dll','imagehlp.dll')
    $result=@()
    foreach ($file in Get-ChildItem -LiteralPath $Root -File | Where-Object { $_.Extension -in '.dll','.exe' }) {
        foreach ($name in @(Get-PortablePeImports $file.FullName)) {
            $kind = 'app-local'
            if ($name -in $system -or $name -in @('bcryptprimitives.dll','xmllite.dll') -or $name -match '^api-ms-win-(core|crt|security|eventing|service|ntuser|downlevel)-[a-z0-9-]+\.dll$') { $kind='windows-system' }
            elseif (!(Test-Path -LiteralPath (Join-Path $Root $name) -PathType Leaf)) { throw "Unresolved PE import: $($file.Name) -> $name" }
            $result += @{file=$file.Name; import=$name; resolution=$kind}
        }
    }
    if (!(Test-Path -LiteralPath (Join-Path $Root 'pdfium.dll'))) { throw 'Dynamic dependency pdfium.dll is required' }
    return $result
}
