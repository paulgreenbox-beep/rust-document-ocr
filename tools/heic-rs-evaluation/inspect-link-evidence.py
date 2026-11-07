"""Windows final-link analysis. Raw receipts contain local paths: keep them private.

Run after linker_capture.rs and under the build process\'s LIB environment.
This is bounded input/symbol/map/import evidence, not a whole-program SBOM or
redistribution approval. Missing temporary objects remain explicitly unresolved.
"""
import argparse
import ctypes
from ctypes import wintypes
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess


def sha(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def argv_windows(text):
    shell = ctypes.WinDLL('shell32', use_last_error=True)
    kernel = ctypes.WinDLL('kernel32', use_last_error=True)
    shell.CommandLineToArgvW.argtypes = [wintypes.LPCWSTR, ctypes.POINTER(ctypes.c_int)]
    shell.CommandLineToArgvW.restype = ctypes.POINTER(wintypes.LPWSTR)
    kernel.LocalFree.argtypes = [wintypes.HLOCAL]
    kernel.LocalFree.restype = wintypes.HLOCAL
    count = ctypes.c_int()
    pointer = shell.CommandLineToArgvW('link.exe ' + text, ctypes.byref(count))
    if not pointer:
        raise ctypes.WinError(ctypes.get_last_error())
    try:
        return [pointer[index] for index in range(1, count.value)]
    finally:
        kernel.LocalFree(ctypes.cast(pointer, wintypes.HLOCAL))


parser = argparse.ArgumentParser()
parser.add_argument('build', type=Path)
parser.add_argument('--llvm', type=Path, required=True)
parser.add_argument('--output', type=Path)
args = parser.parse_args()
link = args.build / 'link'
invocation = json.loads((link / 'invocation.json').read_text(encoding='utf-8'))
arguments = []
responses = []
for argument in invocation['arguments']:
    exact = b''.join(unit.to_bytes(2, 'little') for unit in argument['utf16']).decode('utf-16-le')
    assert exact == argument['display']
    if exact.startswith('@'):
        response = link / ('argument-' + str(argument['index']) + '.rsp.raw')
        data = response.read_bytes()
        if data.startswith((b'\xff\xfe', b'\xfe\xff')):
            encoding = 'utf-16'
        elif len(data) % 2 == 0 and data[1::2].count(0) > len(data) // 8:
            encoding = 'utf-16-le'
        else:
            encoding = 'utf-8-sig'
        text = data.decode(encoding)
        if '\x00' in text:
            raise RuntimeError('NUL after response decoding; refuse truncated argv')
        # rustc's MSVC response format is one quoted argument per line.
        # CommandLineToArgvW does not treat LF as a delimiter. Refuse an
        # unfamiliar grammar instead of silently accepting a truncated argv.
        expanded = []
        for line in text.split('\n'):
            line = line.removesuffix('\r')
            if not line.strip():
                continue
            if not (line.startswith('"') and line.endswith('"') and line.count('"') == 2) or any(ord(c) < 32 for c in line):
                raise RuntimeError('Unexpected rustc MSVC response line')
            values = argv_windows(line)
            if len(values) != 1 or '\r' in values[0] or '\n' in values[0]:
                raise RuntimeError('Response line must encode exactly one argument')
            expanded.extend(values)
        if any(value.startswith('@') for value in expanded):
            raise RuntimeError('Nested response file not captured; refuse incomplete proof')
        arguments.extend(expanded)
        responses.append({'file': response.name, 'bytes': len(data), 'sha256': sha(response), 'encoding': encoding})
    else:
        arguments.append(exact)

search = [Path(value.split(':', 1)[1]) for value in arguments if value.lower().startswith('/libpath:')]
search += [Path(value) for value in os.environ.get('LIB', '').split(';') if value]
inputs, unresolved, switches = [], [], []
for value in arguments:
    original = value
    if value.lower().startswith(('/defaultlib:', '/wholearchive:')):
        value = value.split(':', 1)[1]
        if not Path(value).suffix:
            value += '.lib'
    elif value.startswith('/'):
        switches.append(value)
        continue
    if Path(value).suffix.lower() not in ('.lib', '.rlib', '.o', '.obj'):
        continue
    path = Path(value)
    if not path.is_absolute():
        path = next((root / path for root in [Path(invocation['cwd']), *search] if (root / path).is_file()), path)
    if not path.is_file():
        unresolved.append(original)
        continue
    inputs.append({'path': str(path.resolve()), 'name': path.name, 'size': path.stat().st_size, 'sha256': sha(path)})

banned = re.compile(r'(?:libheif|libde265|(?:^|[ /\\])heif\.lib|(?:^|[ /\\])de265\.lib)', re.I)
native_symbol = re.compile(rb'(?:^|[\s:])(?:__imp_)?_?((?:heif|de265)_[A-Za-z0-9_]+)(?:\s|$)')
if not inputs or not any(Path(item['path']).suffix.lower() == '.rlib' for item in inputs):
    raise RuntimeError('No Rust link inputs resolved; refuse empty absence proof')
archive_scans = []
for item in inputs:
    if Path(item['path']).suffix.lower() not in ('.lib', '.rlib'):
        continue
    output = subprocess.run([str(args.llvm / 'llvm-nm.exe'), '--defined-only', '--extern-only', '--format=posix', item['path']],
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=90, check=False)
    matches = sorted({match.group(1).decode('ascii') for line in output.stdout.splitlines()
                      for match in [native_symbol.search(line)] if match})
    archive_scans.append({'archive': item['name'], 'sha256': item['sha256'], 'exit_code': output.returncode,
                          'symbol_lines': len(output.stdout.splitlines()), 'old_heif_symbols': matches,
                          'stderr_sha256': hashlib.sha256(output.stderr).hexdigest(),
                          'stderr_bytes': len(output.stderr)})

map_bytes = (args.build / 'ocr-pipeline.map').read_bytes()
stdout = (link / 'link.stdout.raw').read_bytes()
map_hits = [line.decode('utf-8', 'replace') for line in map_bytes.splitlines() if banned.search(line.decode('utf-8', 'replace'))]
verbose_hits = [line.decode('utf-8', 'replace') for line in stdout.splitlines() if banned.search(line.decode('utf-8', 'replace'))]
exe = args.build / 'artifacts/ocr-pipeline.exe'
pe = subprocess.run([str(args.llvm / 'llvm-readobj.exe'), '--file-headers', '--coff-imports', str(exe)],
                    stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=30, check=True)
pe_path = args.build / 'pe-imports-private.txt'
if pe_path.exists():
    assert pe_path.read_bytes() == pe.stdout
else:
    pe_path.write_bytes(pe.stdout)
pe_text = pe.stdout.decode('utf-8', 'replace')
exe_bytes = exe.read_bytes()
dll_literals = {}
for name in ('heif.dll', 'libheif.dll', 'libde265.dll', 'de265.dll'):
    dll_literals[name] = {'ascii': name.encode() in exe_bytes.lower(),
                          'utf16le': name.encode('utf-16-le') in exe_bytes.lower()}
report = {'schema_version': 1, 'exe_sha256': sha(exe), 'response_files': responses, 'argument_count': len(arguments),
          'resolved_inputs': inputs, 'unresolved_inputs': unresolved, 'search_paths': [str(p) for p in search],
          'explicit_old_inputs': [item for item in inputs if banned.search(item['name'])], 'archive_scans': archive_scans,
          'map_old_hits': map_hits, 'verbose_old_hits': verbose_hits,
          'old_dll_literal_matches': dll_literals, 'pe_old_import_match': bool(banned.search(pe_text)),
          'map_sha256': sha(args.build / 'ocr-pipeline.map'), 'pe_output_sha256': hashlib.sha256(pe.stdout).hexdigest(),
          'link_exit': json.loads((link / 'exit.json').read_text())['exit_code']}
destination = args.output or args.build / 'LINK_ANALYSIS_PRIVATE.json'
with destination.open('x', encoding='utf-8') as stream:
    json.dump(report, stream, indent=2)
print(json.dumps({'inputs': len(inputs), 'unresolved': len(unresolved), 'old_inputs': len(report['explicit_old_inputs']),
                  'archives': len(archive_scans), 'nm_failures': sum(row['exit_code'] != 0 for row in archive_scans),
                  'old_symbol_archives': sum(bool(row['old_heif_symbols']) for row in archive_scans),
                  'map_old_hits': len(map_hits), 'verbose_old_hits': len(verbose_hits),
                  'pe_old_import_match': report['pe_old_import_match'], 'link_exit': report['link_exit']}))
