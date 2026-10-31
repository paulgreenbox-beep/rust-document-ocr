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
    if not dest.resolve().is_relative_to(notice_root.resolve()):
        raise ValueError("Notice destination resolves outside output root")
    # Avoid collision while retaining deterministic output.
    if dest.exists() and sha256(dest) != sha256(src):
        name = f"{safe_id(label)}-{sha256(src)[:12]}-{safe_id(src.name)}"
        dest = notice_root / name
        if not dest.resolve().is_relative_to(notice_root.resolve()):
            raise ValueError("Notice destination resolves outside output root")
    if not dest.exists():
        # Exclusive creation also refuses dangling symlinks and race-time files.
        with src.open("rb") as source, dest.open("xb") as target:
            shutil.copyfileobj(source, target)
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


def vcpkg_inventory(root: Path, output: Path, notices: Path, *,
                    repo: Path | None = None,
                    triplet: str = "x64-windows-static-md") -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    """Collect installed-state evidence, never infer final executable closure.

    Receipt association/validation is shared with the metadata-only replay API.
    All selected-triplet packages and both archive configurations are retained;
    Cargo link requests are annotations, not proof of retained archive members.
    """
    from vcpkg_inventory import (
        ensure_output_separate, parse_status, safe_relative, validate_metadata,
    )

    root, output, notices = root.resolve(), output.resolve(), notices.resolve()
    ensure_output_separate(root, output)
    ensure_output_separate(root, notices)
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.+-]*", triplet):
        raise ValueError("Unsafe triplet identity")
    installed = root / "installed"
    info = installed / "vcpkg" / "info"
    status_path = installed / "vcpkg" / "status"
    if any(not p.resolve().is_relative_to(root) for p in (installed, info, status_path)):
        raise ValueError("Installed metadata resolves outside input root")
    if not info.is_dir() or not status_path.is_file():
        raise ValueError("vcpkg installed metadata directories/status not found")
    pending = installed / "vcpkg" / "updates"
    if pending.exists() and any(pending.iterdir()):
        raise ValueError("Pending vcpkg status updates")
    status_text = status_path.read_text(encoding="utf-8-sig")
    cores, _ = parse_status(status_text)
    selected = {p for p, t in cores if t == triplet}
    listing_paths = sorted(info.glob(f"*_{triplet}.list"))
    if any(not p.resolve().is_relative_to(installed.resolve()) for p in listing_paths):
        raise ValueError("Installed list resolves outside input root")
    listings = {p.name: p.read_text(encoding="utf-8-sig") for p in listing_paths}
    # Keep hashes tied to full installed-relative paths, never basename.
    files: dict[str, dict[str, Any]] = {}
    for text in listings.values():
        for line in text.splitlines():
            if not line.strip():
                continue
            rel = safe_relative(line.strip(), triplet).as_posix()
            if line.strip().endswith(("/", "\\")):
                continue
            actual = installed / rel
            if not actual.resolve().is_relative_to(installed.resolve()):
                raise ValueError("Installed file resolves outside input root")
            before = actual.stat()
            measured = sha256(actual)
            after = actual.stat()
            if (before.st_size, before.st_mtime_ns, before.st_ino) != (after.st_size, after.st_mtime_ns, after.st_ino):
                raise ValueError("Installed file changed while hashing")
            files[rel] = {"size": after.st_size, "sha256": measured}
    # Also catch archives added without a .list receipt in either configuration.
    listed_archives = {p for p in files if p.lower().endswith(".lib")
                       and (p.startswith(f"{triplet}/lib/") or p.startswith(f"{triplet}/debug/lib/"))}
    observed_archives = {
        p.relative_to(installed).as_posix()
        for subdir in ("lib", "debug/lib")
        for p in (installed / triplet / subdir).rglob("*")
        if p.is_file() and p.suffix.lower() == ".lib"
    }
    if listed_archives != observed_archives:
        raise ValueError("Orphan/missing installed archive membership")
    receipts: dict[str, dict[str, bytes | None]] = {}
    for port in selected:
        share = installed / triplet / "share" / port
        receipts[port] = {}
        for key, name in (("abi_info", "vcpkg_abi_info.txt"), ("spdx", "vcpkg.spdx.json")):
            path = share / name
            if not path.resolve().is_relative_to(installed.resolve()):
                raise ValueError("Receipt resolves outside input root")
            raw = path.read_bytes() if path.is_file() else None
            rel = path.relative_to(installed).as_posix()
            if raw is not None and rel in files and hashlib.sha256(raw).hexdigest().upper() != files[rel]["sha256"]:
                raise ValueError("Receipt changed after installed-file measurement")
            receipts[port][key] = raw
    result = validate_metadata(status_text, listings, triplet, receipts, files)
    # Recheck metadata and directory membership before copying notices/output.
    if status_path.read_text(encoding="utf-8-sig") != status_text:
        raise ValueError("Installed status changed during collection")
    if {p.name: p.read_text(encoding="utf-8-sig") for p in info.glob(f"*_{triplet}.list")} != listings:
        raise ValueError("Installed list membership/content changed during collection")
    final_archives = {
        p.relative_to(installed).as_posix()
        for subdir in ("lib", "debug/lib")
        for p in (installed / triplet / subdir).rglob("*")
        if p.is_file() and p.suffix.lower() == ".lib"
    }
    if final_archives != observed_archives or (pending.exists() and any(pending.iterdir())):
        raise ValueError("Installed archive membership/status updates changed during collection")
    for port, values in receipts.items():
        for key, name in (("abi_info", "vcpkg_abi_info.txt"), ("spdx", "vcpkg.spdx.json")):
            path = installed / triplet / "share" / port / name
            if (path.read_bytes() if path.is_file() else None) != values[key]:
                raise ValueError("Installed receipt changed during collection")
    requests: dict[str, set[str]] = {}
    build_root = (repo or REPO) / "target" / "release" / "build"
    if build_root.is_dir():
        for build_output in sorted(build_root.glob("*/output")):
            for line in build_output.read_text(encoding="utf-8", errors="replace").splitlines():
                match = re.search(r"rustc-link-lib=(?:(static|dylib|framework|verbatim)=)?([^:;\s]+)", line)
                if match:
                    requests.setdefault(match[2].lower(), set()).add(match[1] or "unspecified")
    for item in result:
        port = item["id"].removeprefix("vcpkg:")
        copyright_path = installed / triplet / "share" / port / "copyright"
        if not copyright_path.resolve().is_relative_to(installed.resolve()):
            raise ValueError("Notice resolves outside input root")
        notice = copy_notice(copyright_path, notices, f"vcpkg-{port}-{item['version']}") if copyright_path.is_file() else None
        item["notices"] = [notice] if notice else []
        item["source_url"] = f"https://github.com/microsoft/vcpkg/tree/master/ports/{port}"
        item["license_review"] = "Installed-state evidence only; source correspondence, obligations, final link closure and redistribution remain blocked."
        if notice:
            item["evidence"]["notice_sha256"] = sha256(copyright_path)
        item["evidence"]["validation_scope"] = "Current installed files hashed with per-file stability checks; not an atomic snapshot or historical link receipt"
        for archive in item["evidence"]["archives"]:
            name = archive["name"]
            archive["rustc_link_kinds"] = sorted({
                kind for key in (Path(name).stem.lower(), name.lower())
                for kind in requests.get(key, set())
            })
    warnings = []
    if not requests:
        warnings.append({"error": "No saved Cargo link requests; installed archive inventory is not executable link-closure evidence"})
    return result, warnings


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
    parser.add_argument("--vcpkg-triplet", default="x64-windows-static-md")
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
    from vcpkg_inventory import ensure_output_separate
    # Reject output/input overlap before creating directories or copying notices.
    ensure_output_separate(args.vcpkg_root, outdir)
    for directory in [*args.cargo_registry, args.pdfium_dir, args.tessdata_dir, args.redist_dir]:
        if directory is not None:
            ensure_output_separate(directory, outdir)
    outdir.mkdir(parents=True, exist_ok=True)
    if not noticedir.resolve().is_relative_to(outdir):
        raise ValueError("Notice directory resolves outside output root")
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
    vcpkg_items, warnings = vcpkg_inventory(args.vcpkg_root.resolve(), outdir, noticedir,
                                           repo=repo, triplet=args.vcpkg_triplet)
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
            "vcpkg_triplet": args.vcpkg_triplet,
            "vcpkg_inventory_scope": "All installed packages and owned archives of selected triplet; saved Cargo link requests are annotations only",
            "pe_imports": imports,
            "warnings": warnings,
            "important_limit": "Cargo.lock may include optional/target-specific crates; build output is necessary but not sufficient to establish exact linker closure. Static component sha256 is a deterministic aggregate of exact local archive hashes; preserve per-archive hashes in evidence.",
        },
    }
    with manifest_path.open("x", encoding="utf-8") as stream:
        stream.write(json.dumps(manifest, indent=2, ensure_ascii=False) + "\n")
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
