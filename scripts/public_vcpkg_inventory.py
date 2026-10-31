#!/usr/bin/env python3
"""Build a strict, sanitized public projection of the pinned local vcpkg receipt."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from datetime import datetime
from pathlib import Path, PurePosixPath
from typing import Any


SOURCE_LOCAL_MANIFEST_SHA256 = (
    "dedb8078c29714c0b3a56ca83ceb1f258479848319d9a47b5cca204eb87c811a"
)
_HEX_SHA256 = re.compile(r"\A[0-9a-fA-F]{64}\Z")
_TRIPLET = re.compile(r"\A[a-z0-9][a-z0-9-]{0,63}\Z")
_PACKAGE = re.compile(r"\A[a-z0-9][a-z0-9+.-]{0,127}\Z")
_VERSION = re.compile(r"\A[A-Za-z0-9][A-Za-z0-9._+-]{0,127}\Z")
_DISPLAY_VERSION = re.compile(r"\A[A-Za-z0-9][A-Za-z0-9._+-]{0,127}(?:#[0-9]+)?\Z")
_PATH_SEGMENT = re.compile(r"\A[A-Za-z0-9._+ -]+\Z")
_WINDOWS_NUMBERED_DEVICE = re.compile(r"\A(?:com|lpt)[1-9]\Z", re.IGNORECASE)
_LICENSE_TOKEN = re.compile(r"[A-Za-z0-9][A-Za-z0-9.+:-]*|[()]")
_STATUS_FIELDS = ("Version", "Port-Version", "Abi", "Architecture")

_LIMITATIONS = [
    "This is a derivative of a local observation, not a restored historical linker receipt.",
    "The source observation records a bounded payload-hash recheck; this projection did not re-read or re-hash real binaries.",
    "Matching installed-library measurements do not establish which archive members were linked into or retained by the executable.",
    "SPDX license strings are preserved as source metadata only; they are not a legal conclusion or redistribution approval.",
]


class ProjectionError(ValueError):
    """The pinned source receipt does not meet the public projection schema."""


def _fail(message: str) -> None:
    raise ProjectionError(message)


def _mapping(value: Any, label: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        _fail(f"{label} must be an object")
    return value


def _sequence(value: Any, label: str) -> list[Any]:
    if not isinstance(value, list):
        _fail(f"{label} must be an array")
    return value


def _string(value: Any, pattern: re.Pattern[str], label: str) -> str:
    if not isinstance(value, str) or not pattern.fullmatch(value):
        _fail(f"{label} is invalid")
    return value


def _digest(value: Any, label: str) -> str:
    return _string(value, _HEX_SHA256, label)


def _strict_bool(value: Any, expected: bool | None, label: str) -> bool:
    if type(value) is not bool or (expected is not None and value is not expected):
        _fail(f"{label} is invalid")
    return value


def _strict_int(value: Any, label: str, *, minimum: int = 0) -> int:
    if type(value) is not int or value < minimum:
        _fail(f"{label} is invalid")
    return value


def _port_version(value: Any, label: str) -> str | None:
    if value is None:
        return None
    if not isinstance(value, str) or not value.isascii() or not value.isdigit():
        _fail(f"{label} is invalid")
    return value


def _display_version(version: str, port_version: str | None) -> str:
    if port_version is not None and int(port_version) != 0:
        return f"{version}#{port_version}"
    return version


def _parse_license_expression(value: Any) -> str:
    if not isinstance(value, str) or not value or len(value) > 512:
        _fail("license expression is invalid")

    tokens: list[str] = []
    position = 0
    while position < len(value):
        token_match = _LICENSE_TOKEN.match(value, position)
        if token_match is not None:
            tokens.append(token_match.group(0))
            position = token_match.end()
            continue
        if value[position] == " ":
            position += 1
            continue
        _fail("license expression is invalid")

    cursor = 0

    def parse_primary() -> None:
        nonlocal cursor
        if cursor >= len(tokens):
            _fail("license expression is invalid")
        token = tokens[cursor]
        if token == "(":
            cursor += 1
            parse_or_expression()
            if cursor >= len(tokens) or tokens[cursor] != ")":
                _fail("license expression is invalid")
            cursor += 1
        elif token not in {"AND", "OR", "WITH", ")"}:
            cursor += 1
        else:
            _fail("license expression is invalid")

        if cursor < len(tokens) and tokens[cursor] == "WITH":
            cursor += 1
            if cursor >= len(tokens) or tokens[cursor] in {"AND", "OR", "WITH", "(", ")"}:
                _fail("license expression is invalid")
            cursor += 1

    def parse_and_expression() -> None:
        nonlocal cursor
        parse_primary()
        while cursor < len(tokens) and tokens[cursor] == "AND":
            cursor += 1
            parse_primary()

    def parse_or_expression() -> None:
        nonlocal cursor
        parse_and_expression()
        while cursor < len(tokens) and tokens[cursor] == "OR":
            cursor += 1
            parse_and_expression()

    parse_or_expression()
    if cursor != len(tokens):
        _fail("license expression is invalid")
    return value


def _timestamp(value: Any, label: str) -> datetime:
    if not isinstance(value, str) or len(value) > 64:
        _fail(f"{label} is invalid")
    try:
        parsed = datetime.fromisoformat(value)
    except ValueError:
        _fail(f"{label} is invalid")
    if parsed.tzinfo is None or parsed.utcoffset() is None:
        _fail(f"{label} must include a timezone")
    return parsed


def _safe_relative_path(value: Any, triplet: str) -> str:
    if not isinstance(value, str) or len(value) > 512 or "\\" in value:
        _fail("relative path is invalid")
    if value.startswith("/") or "//" in value:
        _fail("relative path is invalid")
    parts = value.split("/")
    if len(parts) < 2 or parts[0] != triplet:
        _fail("relative path does not belong to the selected triplet")
    for part in parts:
        if (
            part in {"", ".", ".."}
            or part.endswith(".")
            or part.startswith(" ")
            or part.endswith(" ")
            or not _PATH_SEGMENT.fullmatch(part)
        ):
            _fail("relative path is invalid")
        device_stem = part.split(".", 1)[0].rstrip(" .").casefold()
        if device_stem in {"con", "prn", "aux", "nul"} or _WINDOWS_NUMBERED_DEVICE.fullmatch(
            device_stem
        ):
            _fail("relative path contains a reserved Windows device name")
    path = PurePosixPath(value)
    if path.as_posix() != value:
        _fail("relative path is invalid")
    return value


def _safe_library_path(value: Any, triplet: str, configuration: str) -> str:
    value = _safe_relative_path(value, triplet)
    parts = value.split("/")
    path = PurePosixPath(value)
    if path.suffix.lower() != ".lib":
        _fail("library path is invalid")
    if configuration == "release":
        valid_layout = len(parts) >= 3 and parts[0] == triplet and parts[1] == "lib"
    elif configuration == "debug":
        valid_layout = (
            len(parts) >= 4
            and parts[0] == triplet
            and parts[1] == "debug"
            and parts[2] == "lib"
        )
    else:
        valid_layout = False
    if not valid_layout:
        _fail("library path does not match its configuration")
    return value


def _package_file_rows(
    value: Any,
    triplet: str,
    package_key: tuple[str, str],
    global_paths: dict[str, tuple[str, str]],
) -> dict[str, dict[str, Any]]:
    rows: dict[str, dict[str, Any]] = {}
    for index, raw in enumerate(_sequence(value, "package files")):
        source_file = _mapping(raw, f"package file {index}")
        path = _safe_relative_path(source_file.get("path"), triplet)
        folded_path = path.casefold()
        if folded_path in global_paths:
            _fail("installed file paths are not globally unique under case folding")
        global_paths[folded_path] = package_key
        if path in rows:
            _fail("package file path is duplicated")
        rows[path] = {
            "size": _strict_int(source_file.get("size"), "package file size", minimum=1),
            "sha256": _digest(source_file.get("sha256"), "package file SHA-256"),
        }
    return rows


def _verify_component_libraries(
    package_files: dict[str, dict[str, Any]],
    release_libraries: list[dict[str, Any]],
    debug_libraries: list[dict[str, Any]],
) -> None:
    archive_rows = release_libraries + debug_libraries
    archive_paths = {row["path"] for row in archive_rows}
    source_library_paths = {
        path for path in package_files if PurePosixPath(path).suffix.casefold() == ".lib"
    }
    if source_library_paths != archive_paths:
        _fail("archive paths do not exactly match the package .lib file set")
    for archive in archive_rows:
        source_file = package_files.get(archive["path"])
        if source_file is None:
            _fail("archive path is absent from the package file inventory")
        if source_file["size"] != archive["size"]:
            _fail("archive size does not match the package file inventory")
        if source_file["sha256"].lower() != archive["sha256"].lower():
            _fail("archive SHA-256 does not match the package file inventory")


def _archive_rows(value: Any, triplet: str, configuration: str) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    seen_paths: set[str] = set()
    for index, raw in enumerate(_sequence(value, f"{configuration} libraries")):
        archive = _mapping(raw, f"{configuration} library {index}")
        path = _safe_library_path(archive.get("path"), triplet, configuration)
        if path in seen_paths:
            _fail("duplicate library path")
        seen_paths.add(path)
        name = archive.get("name")
        if not isinstance(name, str) or name != PurePosixPath(path).name:
            _fail("library basename does not match its relative path")
        if archive.get("configuration") != configuration:
            _fail("library configuration is invalid")
        rows.append(
            {
                "path": path,
                "size": _strict_int(archive.get("size"), "library size", minimum=1),
                "sha256": _digest(archive.get("sha256"), "library SHA-256"),
            }
        )
    return sorted(rows, key=lambda row: row["path"])


def _package_status(package: dict[str, Any], package_name: str, triplet: str) -> dict[str, Any]:
    if package.get("package") != package_name or package.get("triplet") != triplet:
        _fail("package identity does not match its key")
    if not _PACKAGE.fullmatch(package_name) or not _TRIPLET.fullmatch(triplet):
        _fail("package identity is invalid")

    status = _mapping(package.get("status_record"), "package status")
    if status.get("Package") != package_name or status.get("Status") != "install ok installed":
        _fail("package status identity is invalid")
    status_version = _string(status.get("Version"), _VERSION, "installed version")
    port_version = _port_version(status.get("Port-Version"), "installed port version")
    abi = _digest(status.get("Abi"), "installed ABI")
    architecture = _string(status.get("Architecture"), _TRIPLET, "installed architecture")
    if architecture != triplet:
        _fail("package architecture does not match its triplet")
    display_version = _display_version(status_version, port_version)
    if package.get("version") != display_version:
        _fail("package display version does not match its status")
    if _digest(package.get("abi"), "package ABI").lower() != abi.lower():
        _fail("package ABI does not match its status")
    return {
        "Version": status_version,
        "Port-Version": port_version,
        "Abi": abi,
        "Architecture": architecture,
        "display_version": display_version,
    }


def _validate_status(raw: Any, triplet: str, label: str) -> dict[str, str]:
    status = _mapping(raw, label)
    if not set(status).issubset(_STATUS_FIELDS):
        _fail(f"{label} contains unsupported fields")
    result: dict[str, str] = {}
    for field in _STATUS_FIELDS:
        if field not in status:
            continue
        value = status[field]
        if field == "Version":
            result[field] = _string(value, _VERSION, label)
        elif field == "Port-Version":
            parsed = _port_version(value, label)
            if parsed is None:
                _fail(f"{label} is invalid")
            result[field] = parsed
        elif field == "Abi":
            result[field] = _digest(value, label)
        else:
            parsed_triplet = _string(value, _TRIPLET, label)
            if parsed_triplet != triplet:
                _fail(f"{label} has the wrong architecture")
            result[field] = parsed_triplet
    if not {"Version", "Abi", "Architecture"}.issubset(result):
        _fail(f"{label} is incomplete")
    return result


def _source_fact(record: dict[str, Any], key: str, expected: bool) -> None:
    _strict_bool(record.get(key), expected, f"comparison field {key}")


def _validate_summary(manifest: dict[str, Any], derived: dict[str, int | bool]) -> None:
    summary = _mapping(manifest.get("summary"), "source summary")
    int_fields = {
        "installed_ports": derived["package_count"],
        "release_archives": derived["release_library_count"],
        "debug_archives": derived["debug_library_count"],
        "historical_archive_hashes_checked": derived["historical_archive_hash_count"],
        "historical_status_discrepancies": derived["status_discrepancy_count"],
    }
    for field, expected in int_fields.items():
        if _strict_int(summary.get(field), f"summary {field}") != expected:
            _fail(f"source summary {field} is inconsistent")
    bool_fields = {
        "all_historical_archive_hashes_match": derived["all_archive_hashes_match"],
        "all_historical_release_archive_sets_and_hashes_match": derived[
            "all_archive_sets_match"
        ],
    }
    for field, expected in bool_fields.items():
        _strict_bool(summary.get(field), bool(expected), f"summary {field}")


def _validate_public_projection(
    document: dict[str, Any], expected_source_sha256: str | None = None
) -> None:
    expected_top_level = {
        "schema_version",
        "measurement_date",
        "triplet",
        "derived_from_local_observation",
        "historical_link_receipt",
        "distribution_status",
        "source_local_manifest_sha256",
        "measurement_summary",
        "historical_comparison_summary",
        "packages",
        "limitations",
    }
    if set(document) != expected_top_level:
        _fail("public document contains fields outside the allowlist")
    if document["schema_version"] != 1 or type(document["schema_version"]) is not int:
        _fail("public schema version is invalid")
    if not isinstance(document["measurement_date"], str) or not re.fullmatch(
        r"\d{4}-\d{2}-\d{2}", document["measurement_date"]
    ):
        _fail("public measurement date is invalid")
    _string(document["triplet"], _TRIPLET, "public triplet")
    _strict_bool(document["derived_from_local_observation"], True, "public derivation flag")
    _strict_bool(document["historical_link_receipt"], False, "public historical receipt flag")
    if document["distribution_status"] != "blocked":
        _fail("public distribution status is invalid")
    public_source_sha256 = _digest(document["source_local_manifest_sha256"], "public source digest")
    if expected_source_sha256 is not None and public_source_sha256.lower() != expected_source_sha256.lower():
        _fail("public source digest is invalid")
    measurement_summary = _mapping(document["measurement_summary"], "public measurement summary")
    if set(measurement_summary) != {
        "package_count",
        "release_library_count",
        "debug_library_count",
        "prior_payload_recheck_recorded",
        "prior_payload_files_rechecked",
    }:
        _fail("public measurement summary contains fields outside the allowlist")
    for field in (
        "package_count",
        "release_library_count",
        "debug_library_count",
        "prior_payload_files_rechecked",
    ):
        _strict_int(measurement_summary[field], "public measurement count", minimum=1)
    _strict_bool(
        measurement_summary["prior_payload_recheck_recorded"], True, "public prior recheck flag"
    )
    comparison_summary = _mapping(
        document["historical_comparison_summary"], "public historical comparison summary"
    )
    if set(comparison_summary) != {
        "top_level_versions_matched",
        "top_level_versions_checked",
        "release_archive_hashes_matched",
        "release_archive_hashes_checked",
        "release_archive_sets_matched",
        "release_archive_sets_checked",
        "historical_installed_status_discrepancies",
    }:
        _fail("public comparison summary contains fields outside the allowlist")
    for field in comparison_summary:
        _strict_int(comparison_summary[field], "public comparison count")
    public_packages = _sequence(document["packages"], "public packages")
    seen_names: set[str] = set()
    seen_library_paths: set[str] = set()
    release_library_total = 0
    debug_library_total = 0
    for package in public_packages:
        row = _mapping(package, "public package")
        expected_package = {
            "name",
            "triplet",
            "version",
            "port_version",
            "abi",
            "architecture",
            "license",
            "release_libraries",
            "debug_libraries",
            "historical_comparison",
        }
        if set(row) != expected_package:
            _fail("public package contains fields outside the allowlist")
        package_name = _string(row["name"], _PACKAGE, "public package name")
        if package_name in seen_names:
            _fail("duplicate public package identity")
        seen_names.add(package_name)
        _string(row["triplet"], _TRIPLET, "public package triplet")
        if row["triplet"] != document["triplet"]:
            _fail("public package has the wrong triplet")
        version = _string(row["version"], _DISPLAY_VERSION, "public package version")
        if row["port_version"] is not None:
            _port_version(row["port_version"], "public package port version")
            version_base = version.rsplit("#", 1)[0] if "#" in version else version
            if version != _display_version(version_base, row["port_version"]):
                _fail("public package version and port version are inconsistent")
        elif "#" in version:
            _fail("public package version has no matching port version")
        _digest(row["abi"], "public package ABI")
        if row["architecture"] != row["triplet"]:
            _fail("public architecture does not match its triplet")
        _parse_license_expression(row["license"])
        for field, configuration in (("release_libraries", "release"), ("debug_libraries", "debug")):
            paths: set[str] = set()
            libraries = _sequence(row[field], f"public {configuration} libraries")
            if configuration == "release":
                release_library_total += len(libraries)
            else:
                debug_library_total += len(libraries)
            for raw_library in libraries:
                library = _mapping(raw_library, "public library")
                if set(library) != {"path", "size", "sha256"}:
                    _fail("public library contains fields outside the allowlist")
                path = _safe_library_path(library["path"], row["triplet"], configuration)
                folded_path = path.casefold()
                if path in paths or folded_path in seen_library_paths:
                    _fail("duplicate public library path under case folding")
                paths.add(path)
                seen_library_paths.add(folded_path)
                _strict_int(library["size"], "public library size", minimum=1)
                _digest(library["sha256"], "public library SHA-256")
        comparison = _mapping(row["historical_comparison"], "public package comparison")
        if set(comparison) != {
            "top_level_version_match",
            "release_archive_hashes_match",
            "release_archive_set_match",
            "installed_status_mismatch_fields",
        }:
            _fail("public comparison contains fields outside the allowlist")
        _strict_bool(comparison["top_level_version_match"], None, "public version comparison")
        _strict_bool(comparison["release_archive_hashes_match"], None, "public hash comparison")
        _strict_bool(comparison["release_archive_set_match"], None, "public set comparison")
        mismatch_fields = _sequence(
            comparison["installed_status_mismatch_fields"], "public status mismatch fields"
        )
        if any(field not in _STATUS_FIELDS for field in mismatch_fields):
            _fail("public status mismatch field is invalid")
        if len(set(mismatch_fields)) != len(mismatch_fields):
            _fail("public status mismatch fields contain duplicates")
    if _sequence(document["limitations"], "public limitations") != _LIMITATIONS:
        _fail("public limitations differ from the fixed text")
    if measurement_summary["package_count"] != len(public_packages):
        _fail("public package count is inconsistent")
    if measurement_summary["release_library_count"] != release_library_total:
        _fail("public release library count is inconsistent")
    if measurement_summary["debug_library_count"] != debug_library_total:
        _fail("public debug library count is inconsistent")
    if comparison_summary["top_level_versions_checked"] != len(public_packages):
        _fail("public version comparison count is inconsistent")
    if comparison_summary["release_archive_sets_checked"] != len(public_packages):
        _fail("public archive-set comparison count is inconsistent")
    if comparison_summary["top_level_versions_matched"] > comparison_summary["top_level_versions_checked"]:
        _fail("public version comparison count is invalid")
    if comparison_summary["release_archive_hashes_matched"] > comparison_summary["release_archive_hashes_checked"]:
        _fail("public archive hash comparison count is invalid")
    if comparison_summary["release_archive_sets_matched"] > comparison_summary["release_archive_sets_checked"]:
        _fail("public archive-set comparison count is invalid")
    if (
        comparison_summary["historical_installed_status_discrepancies"]
        > comparison_summary["top_level_versions_checked"]
    ):
        _fail("public status discrepancy count is invalid")


def public_projection(manifest: dict[str, Any], source_sha256: str) -> dict[str, Any]:
    """Return an allowlisted public projection of the pinned local receipt.

    No source object is copied recursively. Every output field is assembled below
    from a validated scalar or from fixed public wording.
    """

    if not isinstance(manifest, dict):
        _fail("source manifest must be an object")
    source_sha256 = _digest(source_sha256, "source manifest SHA-256").lower()
    if manifest.get("local_only") is not True:
        _fail("source manifest is not marked local-only")
    if manifest.get("distribution_status") != "blocked":
        _fail("source distribution status is not blocked")

    scope = _mapping(manifest.get("scope"), "source scope")
    triplet = _string(scope.get("triplet"), _TRIPLET, "source triplet")
    if scope.get("all_installed_files_for_selected_triplet_hashed") is not True:
        _fail("source triplet file measurement is incomplete")

    start = _timestamp(manifest.get("started_utc"), "measurement start")
    end = _timestamp(manifest.get("completed_utc"), "measurement end")
    if end < start:
        _fail("measurement interval is invalid")

    package_rows: dict[tuple[str, str], dict[str, Any]] = {}
    global_package_file_paths: dict[str, tuple[str, str]] = {}
    package_file_count = 0
    for index, raw in enumerate(_sequence(manifest.get("packages"), "source packages")):
        package = _mapping(raw, f"source package {index}")
        name = _string(package.get("package"), _PACKAGE, "source package name")
        package_triplet = _string(package.get("triplet"), _TRIPLET, "source package triplet")
        if package_triplet != triplet:
            _fail("source package has the wrong triplet")
        key = (name, package_triplet)
        if key in package_rows:
            _fail("duplicate package identity")
        status = _package_status(package, name, package_triplet)
        files = _package_file_rows(
            package.get("files"), package_triplet, key, global_package_file_paths
        )
        package_file_count += len(files)
        package_rows[key] = {"source": package, "status": status, "files": files}

    component_rows: dict[tuple[str, str], dict[str, Any]] = {}
    for index, raw in enumerate(_sequence(manifest.get("static_components"), "static components")):
        component = _mapping(raw, f"static component {index}")
        component_id = component.get("id")
        if not isinstance(component_id, str) or not component_id.startswith("vcpkg:"):
            _fail("static component identity is invalid")
        name = _string(component_id[len("vcpkg:"):], _PACKAGE, "static component name")
        key = (name, triplet)
        if key in component_rows:
            _fail("duplicate static component identity")
        source_package = package_rows.get(key)
        if source_package is None:
            _fail("static component has no matching installed package")

        component_version = _string(component.get("version"), _DISPLAY_VERSION, "component version")
        package = source_package["source"]
        status = source_package["status"]
        if component_version != status["display_version"]:
            _fail("component version does not match its installed package")
        if component.get("distribution_status") != "blocked":
            _fail("component distribution status is invalid")
        license_expression = _parse_license_expression(component.get("license"))

        evidence = _mapping(component.get("evidence"), "component evidence")
        if _string(evidence.get("triplet"), _TRIPLET, "component triplet") != triplet:
            _fail("component evidence has the wrong triplet")
        evidence_status = _validate_status(
            evidence.get("installed_status"), triplet, "component installed status"
        )
        if evidence_status.get("Version") != status["Version"] or evidence_status.get("Abi", "").lower() != status["Abi"].lower():
            _fail("component status does not match the installed package")
        if evidence_status.get("Port-Version") != status["Port-Version"]:
            _fail("component port version does not match the installed package")

        release_libraries = _archive_rows(evidence.get("archives"), triplet, "release")
        debug_libraries = _archive_rows(evidence.get("debug_archives"), triplet, "debug")
        if not release_libraries or not debug_libraries:
            _fail("component must include release and debug library records")
        _verify_component_libraries(
            source_package["files"], release_libraries, debug_libraries
        )
        component_rows[key] = {
            "name": name,
            "triplet": triplet,
            "version": component_version,
            "port_version": status["Port-Version"],
            "abi": status["Abi"],
            "architecture": status["Architecture"],
            "license": license_expression,
            "release_libraries": release_libraries,
            "debug_libraries": debug_libraries,
        }

    if set(component_rows) != set(package_rows):
        _fail("installed packages and public components do not have the same identities")

    comparison_rows: dict[tuple[str, str], dict[str, Any]] = {}
    total_hash_comparisons = 0
    matched_hash_comparisons = 0
    status_discrepancies = 0
    for index, raw in enumerate(_sequence(manifest.get("historical_comparison"), "historical comparisons")):
        comparison = _mapping(raw, f"historical comparison {index}")
        comparison_id = comparison.get("id")
        if not isinstance(comparison_id, str) or not comparison_id.startswith("vcpkg:"):
            _fail("historical comparison identity is invalid")
        name = _string(comparison_id[len("vcpkg:"):], _PACKAGE, "historical package name")
        key = (name, triplet)
        if key in comparison_rows or key not in component_rows:
            _fail("historical comparison has a missing or duplicate identity")
        package = package_rows[key]
        component = component_rows[key]
        historical_version = _string(
            comparison.get("historical_version"), _DISPLAY_VERSION, "historical version"
        )
        current_version = _string(
            comparison.get("current_version"), _DISPLAY_VERSION, "current comparison version"
        )
        if current_version != component["version"]:
            _fail("current comparison version does not match the measured package")
        top_level_version_match = historical_version == current_version
        _source_fact(comparison, "top_level_version_match", top_level_version_match)

        current_status = _validate_status(
            comparison.get("current_installed_status"), triplet, "current comparison status"
        )
        expected_current_status = {
            field: value
            for field, value in package["status"].items()
            if field in _STATUS_FIELDS and value is not None
        }
        if current_status != expected_current_status:
            _fail("current comparison status does not match the installed package")
        historical_status = _validate_status(
            comparison.get("historical_installed_status"), triplet, "historical comparison status"
        )
        mismatch_fields = [
            field
            for field in _STATUS_FIELDS
            if current_status.get(field) != historical_status.get(field)
            and (field in current_status or field in historical_status)
        ]
        recorded_mismatch_fields = _sequence(
            comparison.get("different_installed_status_fields"), "recorded status mismatch fields"
        )
        if recorded_mismatch_fields != mismatch_fields:
            _fail("recorded status mismatch fields are inconsistent")
        if mismatch_fields:
            status_discrepancies += 1

        release_by_path = {row["path"]: row["sha256"] for row in component["release_libraries"]}
        historical_archives: dict[str, str] = {}
        archive_hashes_match = True
        for archive_index, raw_archive in enumerate(
            _sequence(comparison.get("historical_archive_comparisons"), "historical archive comparisons")
        ):
            historical_archive = _mapping(raw_archive, f"historical archive comparison {archive_index}")
            path = _safe_library_path(historical_archive.get("path"), triplet, "release")
            if path in historical_archives:
                _fail("duplicate historical library path")
            old_sha = _digest(historical_archive.get("historical_sha256"), "historical library SHA-256")
            if path in release_by_path:
                current_sha = _digest(historical_archive.get("current_sha256"), "current library SHA-256")
                if current_sha.lower() != release_by_path[path].lower():
                    _fail("comparison current library SHA-256 does not match measured data")
                path_match = old_sha.lower() == current_sha.lower()
            else:
                if historical_archive.get("current_sha256") is not None:
                    _fail("missing current library must not have a current SHA-256")
                path_match = False
            _strict_bool(historical_archive.get("match"), path_match, "recorded archive comparison")
            historical_archives[path] = old_sha
            total_hash_comparisons += 1
            if path_match:
                matched_hash_comparisons += 1
            else:
                archive_hashes_match = False

        if not historical_archives:
            _fail("historical comparison has no release archive records")
        added = sorted(set(release_by_path) - set(historical_archives))
        missing = sorted(set(historical_archives) - set(release_by_path))
        recorded_added = [
            _safe_library_path(path, triplet, "release")
            for path in _sequence(comparison.get("current_release_archives_added"), "added archives")
        ]
        recorded_missing = [
            _safe_library_path(path, triplet, "release")
            for path in _sequence(comparison.get("historical_release_archives_missing"), "missing archives")
        ]
        if sorted(recorded_added) != added or sorted(recorded_missing) != missing:
            _fail("recorded release archive set differences are inconsistent")
        archive_set_match = not added and not missing
        exact_archive_match = archive_hashes_match and archive_set_match
        _source_fact(comparison, "all_historical_archives_match", archive_hashes_match)
        _source_fact(comparison, "exact_release_archive_set_and_hash_match", exact_archive_match)

        comparison_rows[key] = {
            "top_level_version_match": top_level_version_match,
            "release_archive_hashes_match": archive_hashes_match,
            "release_archive_set_match": archive_set_match,
            "installed_status_mismatch_fields": mismatch_fields,
        }

    if set(comparison_rows) != set(component_rows):
        _fail("historical comparisons do not cover every measured package")

    all_archive_hashes_match = matched_hash_comparisons == total_hash_comparisons
    all_archive_sets_match = all(
        row["release_archive_set_match"] and row["release_archive_hashes_match"]
        for row in comparison_rows.values()
    )
    release_library_count = sum(len(row["release_libraries"]) for row in component_rows.values())
    debug_library_count = sum(len(row["debug_libraries"]) for row in component_rows.values())

    metadata_consistency = _mapping(manifest.get("metadata_consistency"), "metadata consistency")
    _strict_bool(
        metadata_consistency.get("all_installed_payload_hashes_rechecked"),
        True,
        "prior payload recheck flag",
    )
    prior_rechecked_files = _strict_int(
        metadata_consistency.get("installed_payload_files_rechecked"), "prior rechecked file count", minimum=1
    )
    installed_files_hashed = _strict_int(
        _mapping(manifest.get("summary"), "source summary").get("installed_files_hashed"),
        "installed file count",
        minimum=1,
    )
    if prior_rechecked_files != installed_files_hashed:
        _fail("prior payload recheck count is inconsistent")
    if package_file_count != installed_files_hashed:
        _fail("package file inventories do not match the installed-file count")

    derived: dict[str, int | bool] = {
        "package_count": len(component_rows),
        "release_library_count": release_library_count,
        "debug_library_count": debug_library_count,
        "historical_archive_hash_count": total_hash_comparisons,
        "all_archive_hashes_match": all_archive_hashes_match,
        "all_archive_sets_match": all_archive_sets_match,
        "status_discrepancy_count": status_discrepancies,
    }
    _validate_summary(manifest, derived)

    public_packages: list[dict[str, Any]] = []
    for key in sorted(component_rows):
        component = component_rows[key]
        public_packages.append(
            {
                "name": component["name"],
                "triplet": component["triplet"],
                "version": component["version"],
                "port_version": component["port_version"],
                "abi": component["abi"],
                "architecture": component["architecture"],
                "license": component["license"],
                "release_libraries": component["release_libraries"],
                "debug_libraries": component["debug_libraries"],
                "historical_comparison": comparison_rows[key],
            }
        )

    document = {
        "schema_version": 1,
        "measurement_date": start.date().isoformat(),
        "triplet": triplet,
        "derived_from_local_observation": True,
        "historical_link_receipt": False,
        "distribution_status": "blocked",
        "source_local_manifest_sha256": source_sha256,
        "measurement_summary": {
            "package_count": len(component_rows),
            "release_library_count": release_library_count,
            "debug_library_count": debug_library_count,
            "prior_payload_recheck_recorded": True,
            "prior_payload_files_rechecked": prior_rechecked_files,
        },
        "historical_comparison_summary": {
            "top_level_versions_matched": sum(
                1 for row in comparison_rows.values() if row["top_level_version_match"]
            ),
            "top_level_versions_checked": len(comparison_rows),
            "release_archive_hashes_matched": matched_hash_comparisons,
            "release_archive_hashes_checked": total_hash_comparisons,
            "release_archive_sets_matched": sum(
                1
                for row in comparison_rows.values()
                if row["release_archive_set_match"] and row["release_archive_hashes_match"]
            ),
            "release_archive_sets_checked": len(comparison_rows),
            "historical_installed_status_discrepancies": status_discrepancies,
        },
        "packages": public_packages,
        "limitations": list(_LIMITATIONS),
    }
    _validate_public_projection(document, source_sha256)
    return document


def _build_argument_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", required=True, help="local JSON source manifest")
    parser.add_argument("--output", required=True, help="new public JSON output path")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = _build_argument_parser().parse_args(argv)
    source_path = Path(args.input)
    output_path = Path(args.output)
    sidecar_path = output_path.with_suffix(output_path.suffix + ".sha256")
    try:
        if output_path.exists() or sidecar_path.exists():
            print("error: output or checksum sidecar already exists", file=sys.stderr)
            return 2
        source_bytes = source_path.read_bytes()
        source_sha256 = hashlib.sha256(source_bytes).hexdigest()
        if source_sha256 != SOURCE_LOCAL_MANIFEST_SHA256:
            print("error: source digest does not match the pinned local observation", file=sys.stderr)
            return 2
        manifest = json.loads(source_bytes.decode("utf-8"))
        document = public_projection(manifest, source_sha256)
        output_bytes = (json.dumps(document, ensure_ascii=False, indent=2) + "\n").encode("utf-8")
        output_sha256 = hashlib.sha256(output_bytes).hexdigest()
        sidecar_bytes = f"{output_sha256}  {output_path.name}\n".encode("ascii")
        if output_path.resolve(strict=False) == source_path.resolve(strict=False):
            print("error: output path aliases the source input", file=sys.stderr)
            return 2
        with output_path.open("xb") as output_file:
            output_file.write(output_bytes)
        with sidecar_path.open("xb") as sidecar_file:
            sidecar_file.write(sidecar_bytes)
    except ProjectionError:
        print("error: source manifest failed the public projection schema", file=sys.stderr)
        return 2
    except (OSError, UnicodeDecodeError, json.JSONDecodeError):
        print("error: source read or new output creation failed", file=sys.stderr)
        return 2
    print(f"public projection SHA-256: {output_sha256}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
