"""Pure vcpkg receipt validation; supplied payload hashes are not remeasured here.

No filesystem writes, subprocesses, import-time collection, or release clearance.
"""

from __future__ import annotations

import hashlib
import json
import re
from pathlib import Path, PurePosixPath
from typing import Any


def _identity(value: str) -> str:
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.+-]*", value):
        raise ValueError("Unsafe package or triplet identity")
    return value


def parse_status(text: str) -> tuple[dict[tuple[str, str], dict[str, str]], list[dict[str, str]]]:
    cores, features = {}, []
    for stanza in re.split(r"\r?\n\s*\r?\n", text.strip().lstrip("\ufeff")):
        fields: dict[str, str] = {}
        previous = None
        for line in stanza.splitlines():
            if line[:1].isspace() and previous:
                fields[previous] += "\n" + line.strip()
            elif ":" in line:
                previous, value = line.split(":", 1)
                if previous in fields:
                    raise ValueError("Duplicate field in installed status")
                fields[previous] = value.strip()
        if fields.get("Status") != "install ok installed":
            continue
        if not fields.get("Package") or not fields.get("Architecture"):
            raise ValueError("Installed status missing package or triplet")
        key = (_identity(fields["Package"]), _identity(fields["Architecture"]))
        if fields.get("Feature"):
            _identity(fields["Feature"])
            features.append(dict(fields))
        else:
            if key in cores:
                raise ValueError(f"Duplicate/conflicting core status: {key[0]}:{key[1]}")
            if not fields.get("Version"):
                raise ValueError(f"Core status missing Version: {key[0]}:{key[1]}")
            if "Port-Version" in fields and not re.fullmatch(r"[0-9]+", fields["Port-Version"]):
                raise ValueError("Invalid Port-Version")
            cores[key] = dict(fields)
    for feature in features:
        if (feature["Package"], feature["Architecture"]) not in cores:
            raise ValueError("Feature has no matching core status")
    return cores, features


def version_of(status: dict[str, str]) -> str:
    suffix = status.get("Port-Version", "0")
    return status["Version"] + (f"#{suffix}" if suffix != "0" else "")


def safe_relative(value: str, prefix: str | None = None) -> PurePosixPath:
    value = value.replace("\\", "/")
    rel = PurePosixPath(value)
    if (not value or rel.is_absolute() or not rel.parts
            or any(p == ".." or ":" in p or "\x00" in p for p in rel.parts)):
        raise ValueError("Unsafe relative receipt path")
    for part in rel.parts:
        if (part.endswith((".", " ")) or any(c in part for c in '<>"|?*')
                or re.fullmatch(r"(?i)(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])", part.split(".", 1)[0])):
            raise ValueError("Windows-ambiguous relative receipt path")
    if prefix and rel.parts[0] != prefix:
        raise ValueError("Receipt path outside selected triplet")
    return rel


def classify_configuration(relative: str, triplet: str) -> str:
    parts = safe_relative(relative, triplet).parts
    if parts[1:3] == ("debug", "lib"):
        return "debug"
    if parts[1:2] == ("lib",):
        return "release"
    return "other"


def ensure_output_separate(root: Path, output: Path) -> None:
    if output.resolve().is_relative_to(root.resolve()):
        raise ValueError("Output must be outside the existing input installation")


def validate_list_set(expected: set[str], observed: set[str]) -> None:
    if expected != observed:
        raise ValueError("Stale/orphan/missing installed lists")


def release_aggregate(archives: list[dict[str, Any]]) -> str:
    """Legacy digest: sorted basename:UPPERCASE_SHA256, LF, no trailing LF."""
    names = [a["name"] for a in archives]
    if len({n.casefold() for n in names}) != len(names):
        raise ValueError("Legacy release aggregate cannot express duplicate basenames")
    blob = "\n".join(f"{a['name']}:{a['sha256'].upper()}" for a in sorted(archives, key=lambda a: a["name"]))
    return hashlib.sha256(blob.encode("utf-8")).hexdigest().upper()


def _result(status: str, reason: str) -> dict[str, str]:
    return {"status": status, "reason": reason}


def validate_receipts(port: str, triplet: str, status: dict[str, str],
                      receipts: dict[str, bytes | None], files: dict[str, dict[str, Any]]) -> dict[str, Any]:
    """Validate actual receipt bytes against this core and supplied file hashes.

    Mismatches raise ValueError. Missing evidence is explicitly unavailable, not
    a positive check. BUILD_INFO is normally not part of the installed payload.
    """
    abi_raw = receipts.get("abi_info")
    abi = status.get("Abi")
    abi_check = _result("UNAVAILABLE", "ABI receipt or core ABI is absent")
    if abi_raw is not None and abi:
        if hashlib.sha256(abi_raw).hexdigest() != abi.lower():
            raise ValueError(f"ABI receipt differs from own core status: {port}:{triplet}")
        abi_check = _result("PASS", "SHA256 of supplied ABI receipt equals own core ABI")
    result: dict[str, Any] = {
        "abi": abi_check,
        "spdx_identity": _result("UNAVAILABLE", "SPDX receipt is absent"),
        "spdx_files": [],
        "build_info": _result("NOT CHECKED", "BUILD_INFO receipt is absent"),
        "license": "NOASSERTION",
    }
    spdx_raw = receipts.get("spdx")
    if spdx_raw is None:
        return result
    spdx = json.loads(spdx_raw)
    packages = {}
    for package in spdx.get("packages", []):
        ident = package.get("SPDXID")
        if ident in packages:
            raise ValueError("Duplicate SPDX package identity")
        packages[ident] = package
    recipe, binary = packages.get("SPDXRef-port", {}), packages.get("SPDXRef-binary", {})
    if (recipe.get("name") != port or recipe.get("versionInfo") not in {status["Version"], version_of(status)}
            or binary.get("name") != f"{port}:{triplet}"
            or (abi and binary.get("versionInfo", "").lower() != abi.lower())):
        raise ValueError(f"SPDX identity/version differs from own core status: {port}:{triplet}")
    result["spdx_identity"] = _result("PASS", "SPDX port/version and binary package/ABI match own core") if abi else _result("UNAVAILABLE", "Core ABI absent; binary ABI cannot be compared")
    result["license"] = recipe.get("licenseConcluded", "NOASSERTION")
    seen = set()
    for entry in spdx.get("files", []):
        if not entry.get("SPDXID", "").startswith("SPDXRef-binary-file-"):
            continue
        relative = safe_relative(entry["fileName"]).as_posix()
        if relative in seen:
            raise ValueError("Duplicate SPDX binary file path")
        seen.add(relative)
        checksums = {v["checksumValue"].lower() for v in entry.get("checksums", []) if v.get("algorithm", "").upper() == "SHA256"}
        if len(checksums) > 1 or any(not re.fullmatch(r"[0-9a-f]{64}", v) for v in checksums):
            raise ValueError("Invalid/conflicting SPDX SHA256")
        expected = next(iter(checksums), None)
        path = f"{triplet}/{relative}"
        actual = files.get(path, {}).get("sha256")
        if relative == "BUILD_INFO":
            raw = receipts.get("build_info")
            actual = hashlib.sha256(raw).hexdigest() if raw is not None else None
        check = {"path": path, **_result("NOT CHECKED", "Payload hash or recorded SHA256 is absent")}
        if expected and actual:
            if actual.lower() != expected:
                raise ValueError(f"SPDX recorded SHA256 mismatch: {port}:{triplet}:{relative}")
            check = {"path": path, **_result("PASS", "SPDX SHA256 equals supplied file hash")}
        result["spdx_files"].append(check)
        if relative == "BUILD_INFO":
            result["build_info"] = dict(check)
    return result


def validate_metadata(status_text: str, listings: dict[str, str], triplet: str,
                      receipts: dict[str, dict[str, bytes | None]],
                      expected_files: dict[str, dict[str, Any]]) -> list[dict[str, Any]]:
    """Validate saved metadata + supplied hashes WITHOUT reopening any payload.

    listings contains the complete selected-triplet .list filename/text set.
    receipts[port] uses abi_info/spdx/build_info byte values (None if absent).
    expected_files maps full installed-relative paths to SHA256 and optional size.
    A returned PASS concerns only these supplied sources, not a fresh file scan.
    """
    _identity(triplet)
    cores, features = parse_status(status_text)
    selected = {p: s for (p, t), s in cores.items() if t == triplet}
    if not selected:
        raise ValueError("No core status for selected triplet")
    chosen, owners = {}, {}
    suffix = f"_{triplet}.list"
    relevant = {name for name in listings if name.endswith(suffix)}
    for port, status in sorted(selected.items()):
        candidates = [name for name in relevant if name.startswith(port + "_")]
        valid = [name for name in candidates if name[len(port) + 1:-len(suffix)] in {status["Version"], version_of(status)}]
        if len(candidates) != 1 or len(valid) != 1:
            raise ValueError(f"Missing/ambiguous/version-mismatched installed list: {port}:{triplet}")
        chosen[port] = valid[0]
        for line in listings[valid[0]].splitlines():
            if not line.strip():
                continue
            # Validate directories too, but shared directory ownership is normal.
            path = safe_relative(line.strip(), triplet).as_posix()
            if line.strip().endswith(("/", "\\")):
                continue
            folded = path.casefold()
            if folded in owners:
                raise ValueError("Multiple owners/duplicate installed regular file")
            owners[folded] = (port, path)
    validate_list_set(set(chosen.values()), relevant)
    normalized = {}
    for path, measured in expected_files.items():
        relative = safe_relative(path, triplet).as_posix()
        if relative.casefold() not in owners or owners[relative.casefold()][1] != relative:
            raise ValueError("Supplied file has no exact installed-list ownership")
        if relative.casefold() in normalized:
            raise ValueError("Duplicate supplied payload path")
        value = measured.get("sha256", "")
        if not re.fullmatch(r"[0-9A-Fa-f]{64}", value):
            raise ValueError("Invalid supplied payload SHA256")
        record = {"path": relative, "sha256": value.upper()}
        if "size" in measured:
            if type(measured["size"]) is not int or measured["size"] < 0:
                raise ValueError("Invalid supplied payload size")
            record["size"] = measured["size"]
        normalized[relative.casefold()] = record
    for folded, (_, path) in owners.items():
        if path.lower().endswith(".lib") and folded not in normalized:
            raise ValueError("Listed library has no supplied payload hash")
    components = []
    for port, status in sorted(selected.items()):
        files = {v["path"]: dict(v) for key, v in normalized.items() if owners[key][0] == port}
        checks = validate_receipts(port, triplet, status, receipts.get(port, {}), files)
        release, debug, other = [], [], []
        for path, measured in sorted(files.items()):
            if not path.lower().endswith(".lib"):
                continue
            config = classify_configuration(path, triplet)
            archive = {**measured, "name": PurePosixPath(path).name, "configuration": config}
            {"release": release, "debug": debug, "other": other}[config].append(archive)
        components.append({
            "id": f"vcpkg:{port}", "version": version_of(status), "license": checks["license"],
            "license_source": "Installed SPDX port metadata; not distribution clearance",
            "distribution_status": "blocked", "sha256": release_aggregate(release),
            "evidence": {
                "triplet": triplet,
                "installed_status": {k: status[k] for k in ("Version", "Port-Version", "Abi", "Architecture") if k in status},
                "installed_features": ["core", *sorted({f["Feature"] for f in features if (f["Package"], f["Architecture"]) == (port, triplet)})],
                "installed_list": chosen[port], "archives": release, "debug_archives": debug, "other_archives": other,
                "receipt_checks": checks,
                "validation_scope": "Supplied receipt bytes and payload hashes only; no payload rehash or historical link claim",
            },
        })
    return components
