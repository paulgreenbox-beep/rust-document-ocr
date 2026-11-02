#!/usr/bin/env python3
"""Replay saved vcpkg metadata without reopening the installed payload.

Only the supplied capture directory is read. This verifies saved receipt bytes
and recorded payload hashes, not a fresh scan, build or historical link receipt.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
from pathlib import Path

from vcpkg_inventory import safe_relative, validate_metadata


def verify_capture(capture: Path, expected_manifest_sha256: str, expected_index_sha256: str) -> dict:
    for value in (expected_manifest_sha256, expected_index_sha256):
        if not re.fullmatch(r"[0-9a-fA-F]{64}", value):
            raise ValueError("Expected digest must be SHA-256")
    capture = capture.resolve()
    index_raw = (capture / "SHA256SUMS.txt").read_bytes()
    if hashlib.sha256(index_raw).hexdigest() != expected_index_sha256.lower():
        raise ValueError("Saved checksum index digest mismatch")
    index = {}
    for line in index_raw.decode("utf-8").splitlines():
        checksum, relative = line.split("  ", 1)
        relative = safe_relative(relative).as_posix()
        if not re.fullmatch(r"[0-9a-fA-F]{64}", checksum) or relative in index:
            raise ValueError("Invalid/duplicate saved checksum index entry")
        index[relative] = checksum.lower()

    def read(relative: str, optional: bool = False) -> bytes | None:
        relative = safe_relative(relative).as_posix()
        if relative not in index:
            if optional:
                return None
            raise ValueError("Required metadata is absent from saved checksum index")
        path = capture / relative
        if not path.resolve().is_relative_to(capture):
            raise ValueError("Saved metadata resolves outside capture root")
        data = path.read_bytes()
        if hashlib.sha256(data).hexdigest() != index[relative]:
            raise ValueError("Saved metadata bytes differ from checksum index")
        return data

    # The checksum index binds the exact local inputs including old collector.
    for relative in index:
        read(relative)
    raw = read("vcpkg-inventory.local.json")
    if hashlib.sha256(raw).hexdigest() != expected_manifest_sha256.lower():
        raise ValueError("Saved local manifest digest mismatch")
    source = json.loads(raw)
    triplet = source["scope"]["triplet"]
    status = read("evidence/vcpkg/installed-status.txt").decode("utf-8-sig")
    suffix = f"_{triplet}.list"
    listing_prefix = "evidence/vcpkg/info/"
    listings = {relative.removeprefix(listing_prefix): read(relative).decode("utf-8-sig")
                for relative in index if relative.startswith(listing_prefix) and relative.endswith(suffix)}
    files, receipts = {}, {}
    for package in source["packages"]:
        port = package["package"]
        if not re.fullmatch(r"[a-z0-9][a-z0-9+.-]*", port) or package["triplet"] != triplet:
            raise ValueError("Saved package identity mismatch")
        receipts[port] = {
            "abi_info": read(f"evidence/vcpkg/share/{port}/vcpkg_abi_info.txt", optional=True),
            "spdx": read(f"evidence/vcpkg/share/{port}/vcpkg.spdx.json", optional=True),
        }
        for item in package["files"]:
            relative = safe_relative(item["path"], triplet).as_posix()
            if relative in files:
                raise ValueError("Repeated saved payload ownership")
            files[relative] = {"size": item["size"], "sha256": item["sha256"]}
    current = validate_metadata(status, listings, triplet, receipts, files)
    historical = json.loads(read("evidence/historical/WINDOWS_PORTABLE_CANDIDATE_MANIFEST.json"))
    old_components = {row["id"]: row for row in historical["static_components"] if row["id"].startswith("vcpkg:")}
    if set(old_components) != {row["id"] for row in current}:
        raise ValueError("Current/historical component membership differs")
    version_matches = status_discrepancies = hash_count = hash_matches = exact_sets = 0
    for row in current:
        old = old_components[row["id"]]
        version_matches += old["version"] == row["version"]
        old_status = old["evidence"]["installed_status"]
        new_status = row["evidence"]["installed_status"]
        status_discrepancies += any(old_status.get(key) != new_status.get(key)
                                    for key in ("Version", "Port-Version", "Abi", "Architecture"))
        old_archives = {a["path"]: a["sha256"].lower() for a in old["evidence"]["archives"]}
        new_archives = {a["path"]: a["sha256"].lower() for a in row["evidence"]["archives"]}
        hash_count += len(old_archives)
        hash_matches += sum(sha == new_archives.get(path) for path, sha in old_archives.items())
        exact_sets += old_archives == new_archives
    checks = [c["evidence"]["receipt_checks"] for c in current]
    return {
        "distribution_status": "blocked", "historical_link_receipt": False,
        "validation_scope": "Saved metadata bytes and recorded hashes only; no installed payload was reopened",
        "source_local_manifest_sha256": hashlib.sha256(raw).hexdigest(),
        "saved_files_hash_verified": len(index), "packages": len(current),
        "release_archives": sum(len(c["evidence"]["archives"]) for c in current),
        "debug_archives": sum(len(c["evidence"]["debug_archives"]) for c in current),
        "historical_release_hashes_checked": hash_count,
        "historical_release_hashes_matched": hash_matches,
        "historical_release_sets_matched": exact_sets,
        "top_level_versions_matched": version_matches,
        "historical_nested_status_discrepancies": status_discrepancies,
        "abi_receipts_passed": sum(c["abi"]["status"] == "PASS" for c in checks),
        "spdx_identities_passed": sum(c["spdx_identity"]["status"] == "PASS" for c in checks),
        "spdx_recorded_payload_hashes_matched": sum(item["status"] == "PASS" for c in checks for item in c["spdx_files"]),
        "build_info_not_checked": sum(c["build_info"]["status"] == "NOT CHECKED" for c in checks),
        "real_payload_files_rehashed": 0,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--capture", required=True, type=Path)
    parser.add_argument("--expected-manifest-sha256", required=True)
    parser.add_argument("--expected-index-sha256", required=True)
    args = parser.parse_args()
    result = verify_capture(args.capture, args.expected_manifest_sha256, args.expected_index_sha256)
    print(json.dumps(result, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
