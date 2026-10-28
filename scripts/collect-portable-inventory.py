#!/usr/bin/env python3
"""Collect local Windows portable-release dependency evidence (maintainer only).

This is an evidence collector, not a licensing decision. It deliberately marks
all discovered components blocked and writes only to reports/windows-portable-local.
No downloads, builds, installs, or packaging are performed.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import sys
import tomllib
from pathlib import Path
from typing import Any


REPO = Path(__file__).resolve().parents[1]
LICENSE_NAMES = re.compile(r"(?i)^(license|licence|copying|notice|copyright)(?:[._-].*)?$")


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest().upper()


def safe_id(value: str) -> str:
    return re.sub(r"[^A-Za-z0-9._+-]+", "-", value).strip("-")


def copy_notice(src: Path, notice_root: Path, label: str) -> str:
    notice_root.mkdir(parents=True, exist_ok=True)
    name = f"{safe_id(label)}-{safe_id(src.name)}"
    dest = notice_root / name
    # Avoid collision while retaining deterministic output.
    if dest.exists() and sha256(dest) != sha256(src):
        name = f"{safe_id(label)}-{sha256(src)[:12]}-{safe_id(src.name)}"
        dest = notice_root / name
    if not dest.exists():
        shutil.copyfile(src, dest)
    return f"licenses/{name}"


def parse_lock(path: Path) -> list[dict[str, Any]]:
    return [
        package for package in tomllib.loads(path.read_text(encoding="utf-8")).get("package", [])
        if package.get("source", "").startswith("registry+")
    ]


def crate_inventory(lock: Path, registry_roots: list[Path], notices: Path) -> list[dict[str, Any]]:
    out = []
    for pkg in parse_lock(lock):
        name, version = pkg["name"], pkg["version"]
        candidates = [root / f"{name}-{version}" for root in registry_roots]
        source = next((p for p in candidates if p.is_dir()), None)
        if source is None:
            out.append({
                "id": f"crate:{name}:{version}", "version": version,
                "source_url": f"https://crates.io/crates/{name}/{version}",
                "license": "NOASSERTION", "distribution_status": "blocked",
                "license_review": "Local Cargo registry source absent; exact published license and notice files not collected.",
                "notices": [], "sha256": pkg.get("checksum", ""),
                "evidence": {"kind": "Cargo.lock registry archive checksum", "checksum": pkg.get("checksum", "")},
            })
            continue
        manifest = source / "Cargo.toml"
        license_expr = "NOASSERTION"
        try:
            # Cargo.toml is TOML; parse only the [package] scalar fields without
            # imposing a non-stdlib dependency on this maintainer helper.
            section = ""
            for line in manifest.read_text(encoding="utf-8").splitlines():
                m = re.match(r"\s*\[([^]]+)\]", line)
                if m:
                    section = m.group(1)
                elif section == "package" and re.match(r"\s*license\s*=", line):
                    raw = line.split("=", 1)[1].strip()
                    try:
                        license_expr = json.loads(raw)
                    except json.JSONDecodeError:
                        license_expr = raw.strip("'\"") or "NOASSERTION"
                    break
        except OSError:
            pass
        found = sorted(p for p in source.iterdir() if p.is_file() and LICENSE_NAMES.match(p.name))
        notice_paths = [copy_notice(p, notices, f"crate-{name}-{version}") for p in found]
        out.append({
            "id": f"crate:{name}:{version}", "version": version,
            "source_url": f"https://crates.io/crates/{name}/{version}",
            "license": license_expr, "distribution_status": "blocked",
            "license_review": "Inventory only: verify license choice, notices, and actual linked/static closure before redistribution.",
            "notices": notice_paths, "sha256": pkg.get("checksum", ""),
            "evidence": {"kind": "Cargo.lock registry archive checksum", "checksum": pkg.get("checksum", ""),
                         "manifest_evidence": f"local-registry/{name}-{version}/Cargo.toml",
                         "local_notice_files": [{"name": p.name, "sha256": sha256(p)} for p in found]},
        })
    return out


def vcpkg_inventory(root: Path, output: Path, notices: Path) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    installed = root / "installed"
    triplet = "x64-windows-static-md"
    info = installed / "vcpkg" / "info"
    share = installed / triplet / "share"
    libdir = installed / triplet / "lib"
    if not info.is_dir() or not libdir.is_dir():
        return [], [{"error": "vcpkg metadata/library directories not found", "root": str(root)}]
    # Filename maps the exact installed package version and triplet. Each list
    # enumerates the package's installed paths; match selected .lib archives.
    list_meta: dict[str, dict[str, Any]] = {}
    installed_status: dict[str, dict[str, str]] = {}
    status_file = installed / "vcpkg" / "status"
    if status_file.is_file():
        for stanza in re.split(r"\r?\n\s*\r?\n", status_file.read_text(encoding="utf-8", errors="replace")):
            fields = {}
            for line in stanza.splitlines():
                if ": " in line and not line[:1].isspace():
                    key, value = line.split(": ", 1)
                    fields[key] = value
            if fields.get("Architecture") == triplet and fields.get("Package") and fields.get("Version"):
                installed_status[fields["Package"]] = fields
    for listing in info.glob(f"*_{triplet}.list"):
        m = re.match(r"(.+)_([^_]+)_" + re.escape(triplet) + r"\.list$", listing.name)
        if not m:
            continue
        port, version = m.group(1), m.group(2)
        status = installed_status.get(port, {})
        if status.get("Version"):
            version = status["Version"] + (f"#{status['Port-Version']}" if status.get("Port-Version") else "")
        for line in listing.read_text(encoding="utf-8", errors="replace").splitlines():
            rel = line.strip().replace("\\", "/")
            if rel.lower().endswith(".lib"):
                list_meta[Path(rel).name.lower()] = {"port": port, "version": version, "listed_path": rel}

    # Read link requests from Cargo build-script output and resolve exact .lib
    # artifacts only in rustc native search paths or the selected triplet lib.
    build_root = REPO / "target" / "release" / "build"
    requests: dict[str, set[str]] = {}
    search_dirs: set[Path] = {libdir}
    if build_root.is_dir():
        for output_file in build_root.glob("*/output"):
            try:
                lines = output_file.read_text(encoding="utf-8", errors="replace").splitlines()
            except OSError:
                continue
            for line in lines:
                m = re.search(r"rustc-link-lib=(?:(static|dylib|framework|verbatim)=)?([^:;\s]+)", line)
                if m:
                    kind, libname = m.group(1) or "unspecified", m.group(2).lower()
                    requests.setdefault(libname, set()).add(kind)
                sm = re.search(r"rustc-link-search=native=(.+)$", line)
                if sm:
                    candidate = Path(sm.group(1).strip().strip('"'))
                    if candidate.is_dir():
                        search_dirs.add(candidate)
    # Build-script output may be absent before a native build. Do not silently
    # substitute all installed libraries: report that limitation explicitly.
    found_libs: dict[str, Path] = {}
    for d in sorted(search_dirs, key=str):
        if d.is_dir():
            for p in d.glob("*.lib"):
                if not requests or p.stem.lower() in requests or p.name.lower() in requests:
                    found_libs.setdefault(p.name.lower(), p)
    by_port: dict[str, dict[str, Any]] = {}
    errors: list[dict[str, Any]] = []
    for filename, libpath in sorted(found_libs.items()):
        meta = list_meta.get(filename)
        if not meta:
            errors.append({"error": "static library has no matching vcpkg installed-list record", "file": str(libpath)})
            continue
        port, version = meta["port"], meta["version"]
        manifest_path = root / "ports" / port / "vcpkg.json"
        port_manifest: dict[str, Any] = {}
        if manifest_path.exists():
            try:
                port_manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            except (OSError, json.JSONDecodeError):
                pass
        port_license = port_manifest.get("license") or "NOASSERTION"
        copyright = share / port / "copyright"
        notice = copy_notice(copyright, notices, f"vcpkg-{port}-{version}") if copyright.is_file() else None
        item = by_port.setdefault(port, {
            "id": f"vcpkg:{port}", "version": version,
            "source_url": port_manifest.get("homepage") or f"https://github.com/microsoft/vcpkg/tree/master/ports/{port}",
            "license": port_license, "distribution_status": "blocked",
            "license_review": "Inventory only: exact installed archive and status recorded; port-manifest license may be from a different port revision. Verify source correspondence, obligations, and final executable closure.",
            "notices": [notice] if notice else [], "sha256": "",
            "evidence": {"triplet": triplet, "installed_status": {k: status.get(k) for k in ("Version", "Port-Version", "Abi", "Architecture") if status.get(k)},
                         "port_manifest_evidence": f"ports/{port}/vcpkg.json", "archives": []},
        })
        link_kinds = sorted({kind for req, kinds in requests.items() if req in {libpath.stem.lower(), libpath.name.lower()} for kind in kinds})
        item["evidence"]["archives"].append({"path": meta["listed_path"], "name": libpath.name, "sha256": sha256(libpath), "rustc_link_kinds": link_kinds})
        if notice:
            item["evidence"]["notice_sha256"] = sha256(notices / Path(notice).name)
    result = list(by_port.values())
    for item in result:
        # A deterministic aggregate digest binds the exact archive set by name/hash.
        blob = "\n".join(f"{a['name']}:{a['sha256']}" for a in sorted(item["evidence"]["archives"], key=lambda x: x["name"]))
        item["sha256"] = hashlib.sha256(blob.encode("utf-8")).hexdigest().upper()
        if not item["notices"]:
            item["license_review"] += " No installed copyright notice found."
    if not requests:
        errors.append({"error": "no rustc-link-lib requests collected from target/release/build/*/output; run a native build before static-archive inventory"})
    return result, errors


def pe_imports(exe: Path) -> list[str]:
    data = exe.read_bytes()
    if len(data) < 0x40 or data[:2] != b"MZ":
        raise ValueError(f"Not a PE executable: {exe}")
    peoff = int.from_bytes(data[0x3C:0x40], "little")
    if data[peoff:peoff + 4] != b"PE\0\0":
        raise ValueError(f"Invalid PE signature: {exe}")
    coff = peoff + 4
    sections_n = int.from_bytes(data[coff + 2:coff + 4], "little")
    opt_size = int.from_bytes(data[coff + 16:coff + 18], "little")
    opt = coff + 20
    magic = int.from_bytes(data[opt:opt + 2], "little")
    dd = opt + (112 if magic == 0x20B else 96)
    import_rva = int.from_bytes(data[dd + 8:dd + 12], "little")
    if not import_rva:
        return []
    section_table = opt + opt_size
    sections = []
    for i in range(sections_n):
        off = section_table + i * 40
        vsize = int.from_bytes(data[off + 8:off + 12], "little")
        va = int.from_bytes(data[off + 12:off + 16], "little")
        rawsize = int.from_bytes(data[off + 16:off + 20], "little")
        rawptr = int.from_bytes(data[off + 20:off + 24], "little")
        sections.append((va, max(vsize, rawsize), rawptr))

    def offset(rva: int) -> int:
        for va, size, raw in sections:
            if va <= rva < va + size:
                return raw + rva - va
        raise ValueError(f"RVA 0x{rva:x} not mapped in PE sections")

    imports = []
    pos = offset(import_rva)
    while pos + 20 <= len(data):
        desc = data[pos:pos + 20]
        if not any(desc):
            break
        name_rva = int.from_bytes(desc[12:16], "little")
        namepos = offset(name_rva)
        end = data.find(b"\0", namepos)
        if end < 0:
            break
        imports.append(data[namepos:end].decode("ascii", errors="replace"))
        pos += 20
    return sorted(set(imports), key=str.lower)


def external_files(kind: str, directory: Path | None, relative: str) -> dict[str, Any] | None:
    if not directory:
        return None
    files = []
    for path in sorted(directory.rglob("*")):
        if path.is_file():
            files.append({"source": str(path.resolve()), "path": f"{relative}/{path.name}", "sha256": sha256(path), "size": path.stat().st_size})
    return {"id": kind, "files": files}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=Path, default=REPO)
    parser.add_argument("--output-root", type=Path, help="Local ignored output directory; must not already contain runtime-manifest.local.json")
    parser.add_argument("--vcpkg-root", type=Path, default=Path(os.environ["VCPKG_ROOT"]) if os.environ.get("VCPKG_ROOT") else None)
    parser.add_argument("--cargo-registry", type=Path, action="append", default=[])
    parser.add_argument("--exe", type=Path)
    parser.add_argument("--pdfium-dir", type=Path)
    parser.add_argument("--tessdata-dir", type=Path)
    parser.add_argument("--redist-dir", type=Path)
    args = parser.parse_args()
    if args.vcpkg_root is None:
        parser.error("supply --vcpkg-root or process-local VCPKG_ROOT")
    repo = args.repo.resolve()
    outdir = (args.output_root or (repo / "reports" / "windows-portable-local")).resolve()
    noticedir = outdir / "notices"
    outdir.mkdir(parents=True, exist_ok=True)
    manifest_path = outdir / "runtime-manifest.local.json"
    if manifest_path.exists():
        raise FileExistsError(f"Refusing to overwrite existing inventory: {manifest_path}")
    if noticedir.exists():
        # Preserve prior evidence. Identical files are reused; nothing is removed.
        pass
    registry_roots = list(args.cargo_registry)
    if not registry_roots:
        cargo_home = Path(os.environ.get("CARGO_HOME", str(Path.home() / ".cargo")))
        registry_roots = list((cargo_home / "registry" / "src").glob("*"))
    crate_items = crate_inventory(repo / "Cargo.lock", registry_roots, noticedir)
    vcpkg_items, warnings = vcpkg_inventory(args.vcpkg_root.resolve(), outdir, noticedir)
    components: list[dict[str, Any]] = []
    for kind, directory, rel in [
        ("external:pdfium", args.pdfium_dir, "pdfium"),
        ("external:tessdata", args.tessdata_dir, "tessdata"),
        ("external:msvc-redist", args.redist_dir, "redist"),
    ]:
        result = external_files(kind, directory, rel)
        if result:
            components.append({
                "id": result["id"], "version": "UNRESOLVED", "source_url": "UNRESOLVED",
                "license": "NOASSERTION", "distribution_status": "blocked",
                "license_review": "Local files hashed only; identify authoritative source, exact version, license terms, and required notices.",
                "files": result["files"], "notices": [],
            })
    imports = pe_imports(args.exe.resolve()) if args.exe else []
    manifest = {
        "schema_version": 1,
        "distribution_status": "blocked",
        "components": components,
        "static_components": crate_items + vcpkg_items,
        "evidence": {
            "purpose": "LOCAL maintainer inventory only; not a release manifest or distribution clearance",
            "repo": str(repo),
            "vcpkg_root": str(args.vcpkg_root.resolve()),
            "vcpkg_triplet": "x64-windows-static-md",
            "pe_imports": imports,
            "warnings": warnings,
            "important_limit": "Cargo.lock may include optional/target-specific crates; build output is necessary but not sufficient to establish exact linker closure. Static component sha256 is a deterministic aggregate of exact local archive hashes; preserve per-archive hashes in evidence.",
        },
    }
    manifest_path.write_text(json.dumps(manifest, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(f"Wrote {manifest_path}")
    print(f"Cargo registry components: {len(crate_items)}; vcpkg static ports: {len(vcpkg_items)}; PE imports: {len(imports)}; status: blocked")
    if warnings:
        print(f"Warnings: {len(warnings)} (see manifest)")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError) as exc:
        print(f"inventory error: {exc}", file=sys.stderr)
        raise SystemExit(2)
