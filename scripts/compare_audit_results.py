#!/usr/bin/env python3
"""Compare two bounded OCR audit result trees without printing OCR text.

Expected layout (the two fixture directories are required):

    <tree>/text-table/result.v1.json
    <tree>/text-table/result.json          # optional, compared if present
    <tree>/text-table/result.docx          # optional, compared if present
    <tree>/text-table/result.xlsx          # optional, compared if present
    <tree>/raster/...

Reports contain relative fixture/file identifiers, structural geometry,
counts, and hashes only. They never contain input paths or recognized text.
Requires Python 3.10+; uses only the standard library.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
import xml.etree.ElementTree as ET
import zipfile
from pathlib import Path
from typing import Any


FIXTURES = ("text-table", "raster")
JSON_FILES = ("result.v1.json", "result.json")
EXPORT_FILES = ("result.docx", "result.xlsx")
IGNORED_XML_ATTRS = {"rsid", "rsidr", "rsidrpr", "rsidrdefault", "rsidrdel", "rsidp", "rsidtr"}


def stable_json_bytes(value: Any) -> bytes:
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode("utf-8")


def digest_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def text_fingerprint(value: str) -> dict[str, Any]:
    raw = value.encode("utf-8")
    return {"characters": len(value), "sha256": digest_bytes(raw)}


def load_json(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as stream:
        value = json.load(stream)
    if not isinstance(value, dict):
        raise ValueError("top_level_not_object")
    return value


def bbox_geometry(value: Any) -> dict[str, Any] | None:
    if not isinstance(value, dict):
        return None
    names = ("x0", "y0", "x1", "y1") if "x0" in value else ("x", "y", "width", "height")
    if not all(name in value for name in names):
        return None
    return {name: value[name] for name in names}


def cell_geometry(cell: Any) -> dict[str, Any]:
    if not isinstance(cell, dict):
        return {}
    return {
        key: cell.get(key)
        for key in ("row", "column", "row_span", "col_span")
        if key in cell
    } | {"bbox": bbox_geometry(cell.get("bbox"))}


def legacy_table_geometry(table: Any) -> dict[str, Any]:
    table = table if isinstance(table, dict) else {}
    rows = table.get("rows") if isinstance(table.get("rows"), list) else []
    cells = [cell for row in rows if isinstance(row, list) for cell in row]
    return {
        "table_type": table.get("table_type"),
        "detector_type": table.get("detector_type"),
        "bbox": bbox_geometry(table.get("bbox")),
        "rows": len(rows),
        "columns": max((len(row) for row in rows if isinstance(row, list)), default=0),
        "cell_count": len(cells),
        "cells": [cell_geometry(cell) for cell in cells],
    }


def grid_geometry(grid: Any) -> dict[str, Any]:
    grid = grid if isinstance(grid, dict) else {}
    cells = grid.get("cells") if isinstance(grid.get("cells"), list) else []
    return {
        "kind": grid.get("kind"),
        "detector_type": grid.get("detector_type"),
        "bbox": bbox_geometry(grid.get("bbox")),
        "rows": grid.get("rows"),
        "columns": grid.get("columns"),
        "cell_count": len(cells),
        "cells": [cell_geometry(cell) for cell in cells],
    }


def extract_geometry(result: dict[str, Any]) -> dict[str, Any]:
    document = result.get("document") if isinstance(result.get("document"), dict) else {}
    pages = document.get("pages") if isinstance(document.get("pages"), list) else []
    grid_pages = result.get("tables") if isinstance(result.get("tables"), list) else []
    grid_by_page = {
        page.get("page_number"): page.get("grids", [])
        for page in grid_pages
        if isinstance(page, dict)
    }
    return {
        "legacy_pages": [
            {
                "page_number": page.get("page_number"),
                "tables": [legacy_table_geometry(table) for table in (page.get("tables") or [])],
            }
            for page in pages
            if isinstance(page, dict)
        ],
        "detected_grids": [
            {
                "page_number": page.get("page_number"),
                "grids": [grid_geometry(grid) for grid in grid_by_page.get(page.get("page_number"), [])],
            }
            for page in pages
            if isinstance(page, dict)
        ],
    }


def cell_texts_from_legacy(page: dict[str, Any]) -> list[str]:
    values: list[str] = []
    for table in (page.get("tables", []) if isinstance(page.get("tables"), list) else []):
        rows = table.get("rows", []) if isinstance(table, dict) else []
        for row in (rows if isinstance(rows, list) else []):
            if isinstance(row, list):
                values.extend(str(cell.get("text", "")) for cell in row if isinstance(cell, dict))
    return values


def cell_texts_from_grids(result: dict[str, Any], page_number: Any) -> list[str]:
    values: list[str] = []
    for page in (result.get("tables", []) if isinstance(result.get("tables"), list) else []):
        if not isinstance(page, dict) or page.get("page_number") != page_number:
            continue
        for grid in (page.get("grids", []) if isinstance(page.get("grids"), list) else []):
            if isinstance(grid, dict):
                values.extend(
                    str(cell.get("text", ""))
                    for cell in grid.get("cells", [])
                    if isinstance(cell, dict)
                )
    return values


def confidence_values(result: dict[str, Any], pages: list[Any]) -> dict[str, Any]:
    page_processing = result.get("page_processing", [])
    processing_by_page = {
        item.get("page_number"): item
        for item in page_processing
        if isinstance(item, dict)
    } if isinstance(page_processing, list) else {}
    page_values = []
    for page in pages:
        if not isinstance(page, dict):
            continue
        page_number = page.get("page_number")
        processing = processing_by_page.get(page_number, {})
        legacy_cells = [
            cell
            for table in (page.get("tables", []) if isinstance(page.get("tables"), list) else [])
            for row in (table.get("rows", []) if isinstance(table, dict) else [])
            if isinstance(row, list)
            for cell in row
            if isinstance(cell, dict)
        ]
        grid_cells = [
            cell
            for grid_page in (result.get("tables", []) if isinstance(result.get("tables"), list) else [])
            if isinstance(grid_page, dict) and grid_page.get("page_number") == page_number
            for grid in (grid_page.get("grids", []) if isinstance(grid_page.get("grids"), list) else [])
            if isinstance(grid, dict)
            for cell in (grid.get("cells", []) if isinstance(grid.get("cells"), list) else [])
            if isinstance(cell, dict)
        ]
        page_values.append({
            "page_number": page_number,
            "ocr_confidence": processing.get("ocr_confidence"),
            "legacy_table_confidence": [
                table.get("confidence") for table in page.get("tables", [])
                if isinstance(page.get("tables"), list) and isinstance(table, dict)
            ],
            "legacy_cell_confidence": [cell.get("confidence") for cell in legacy_cells],
            "grid_confidence": [
                grid.get("confidence")
                for grid_page in result.get("tables", [])
                if isinstance(result.get("tables"), list)
                and isinstance(grid_page, dict)
                and grid_page.get("page_number") == page_number
                for grid in grid_page.get("grids", [])
                if isinstance(grid, dict)
            ],
            "grid_cell_confidence": [cell.get("confidence") for cell in grid_cells],
        })
    return {"pages": page_values}


def warning_fingerprints(warnings: Any) -> list[dict[str, Any]]:
    if not isinstance(warnings, list):
        return []
    result = []
    for warning in warnings:
        if isinstance(warning, dict):
            kind = warning.get("kind") or warning.get("code") or "unknown"
            detail = warning.get("message") or warning.get("detail") or ""
            result.append({"kind": kind, "detail_sha256": digest_bytes(stable_json_bytes(detail))})
        else:
            result.append({"kind": "unstructured", "detail_sha256": digest_bytes(stable_json_bytes(warning))})
    return sorted(result, key=lambda item: (str(item["kind"]), item["detail_sha256"]))


def extract_ocr(result: dict[str, Any]) -> dict[str, Any]:
    document = result.get("document") if isinstance(result.get("document"), dict) else {}
    pages = document.get("pages") if isinstance(document.get("pages"), list) else []
    processing = result.get("page_processing", [])
    processing_by_page = {
        item.get("page_number"): item
        for item in processing
        if isinstance(item, dict)
    } if isinstance(processing, list) else {}
    page_ocr = []
    for page in pages:
        if not isinstance(page, dict):
            continue
        number = page.get("page_number")
        page_text = str(page.get("text", ""))
        legacy_cells = cell_texts_from_legacy(page)
        grid_cells = cell_texts_from_grids(result, number)
        route = processing_by_page.get(number, {})
        page_ocr.append({
            "page_number": number,
            "page_text": text_fingerprint(page_text),
            "legacy_cell_values": {
                "count": len(legacy_cells),
                "sha256": digest_bytes(stable_json_bytes(legacy_cells)),
            },
            "grid_cell_values": {
                "count": len(grid_cells),
                "sha256": digest_bytes(stable_json_bytes(grid_cells)),
            },
            "route": {
                "processing_mode": route.get("processing_mode"),
                "language": route.get("language"),
                "native_text_quality": route.get("native_text_quality"),
            },
        })
    return {
        "page_order": [page.get("page_number") for page in pages if isinstance(page, dict)],
        "pages": page_ocr,
        "confidence": confidence_values(result, pages),
        "warnings": warning_fingerprints(result.get("warnings", [])),
    }


def flatten_diff(left: Any, right: Any, path: str = "$") -> list[dict[str, Any]]:
    if type(left) is not type(right):
        return [{"field": path, "baseline": left, "candidate": right}]
    if isinstance(left, dict):
        differences = []
        for key in sorted(left.keys() | right.keys()):
            child = f"{path}.{key}"
            if key not in left:
                differences.append({"field": child, "baseline": None, "candidate": right[key]})
            elif key not in right:
                differences.append({"field": child, "baseline": left[key], "candidate": None})
            else:
                differences.extend(flatten_diff(left[key], right[key], child))
        return differences
    if isinstance(left, list):
        differences = []
        max_length = max(len(left), len(right))
        for index in range(max_length):
            child = f"{path}[{index}]"
            if index >= len(left):
                differences.append({"field": child, "baseline": None, "candidate": right[index]})
            elif index >= len(right):
                differences.append({"field": child, "baseline": left[index], "candidate": None})
            else:
                differences.extend(flatten_diff(left[index], right[index], child))
        return differences
    if left != right:
        return [{"field": path, "baseline": left, "candidate": right}]
    return []


def normalize_xml(data: bytes) -> bytes:
    root = ET.fromstring(data)
    for element in root.iter():
        for key in list(element.attrib):
            local_name = key.rsplit("}", 1)[-1].lower()
            if local_name in IGNORED_XML_ATTRS or local_name.startswith("rsid"):
                del element.attrib[key]
    xml_text = ET.tostring(root, encoding="unicode")
    canonical = ET.canonicalize(xml_data=xml_text, with_comments=False, strip_text=False)
    return canonical.encode("utf-8")


def docx_fingerprint(path: Path) -> dict[str, Any]:
    with zipfile.ZipFile(path) as archive:
        data = archive.read("word/document.xml")
    normalized = normalize_xml(data)
    return {"parts": {"word/document.xml": digest_bytes(normalized)}}


def xlsx_fingerprint(path: Path) -> dict[str, Any]:
    with zipfile.ZipFile(path) as archive:
        names = set(archive.namelist())
        selected = {"xl/workbook.xml"}
        selected.update(name for name in names if re.fullmatch(r"xl/worksheets/[^/]+\.xml", name))
        if "xl/sharedStrings.xml" in names:
            selected.add("xl/sharedStrings.xml")
        parts: dict[str, str] = {}
        for name in sorted(selected):
            parts[name] = digest_bytes(normalize_xml(archive.read(name)))
    return {"parts": parts}


def compare_optional_file(
    baseline: Path,
    candidate: Path,
    kind: str,
) -> dict[str, Any]:
    left_exists, right_exists = baseline.is_file(), candidate.is_file()
    if not left_exists and not right_exists:
        return {"status": "not_present_both", "unexpected": False}
    if left_exists != right_exists:
        return {
            "status": "missing_on_one_side",
            "baseline_present": left_exists,
            "candidate_present": right_exists,
            "unexpected": True,
        }
    try:
        left = docx_fingerprint(baseline) if kind == "docx" else xlsx_fingerprint(baseline)
        right = docx_fingerprint(candidate) if kind == "docx" else xlsx_fingerprint(candidate)
    except (OSError, KeyError, zipfile.BadZipFile, ET.ParseError, ValueError):
        return {"status": "invalid_export", "unexpected": True}
    differences = flatten_diff(left, right)
    return {
        "status": "equal" if not differences else "different",
        "baseline": left,
        "candidate": right,
        "differences": differences,
        "unexpected": bool(differences),
    }


def compare_json_file(baseline: Path, candidate: Path, filename: str) -> dict[str, Any]:
    left_exists, right_exists = baseline.is_file(), candidate.is_file()
    if not left_exists and not right_exists:
        return {"status": "missing_required_both" if filename == "result.v1.json" else "not_present_both", "unexpected": filename == "result.v1.json"}
    if left_exists != right_exists:
        return {"status": "missing_on_one_side", "baseline_present": left_exists, "candidate_present": right_exists, "unexpected": True}
    try:
        left, right = load_json(baseline), load_json(candidate)
    except (OSError, json.JSONDecodeError, ValueError):
        return {"status": "invalid_json", "unexpected": True}
    if filename == "result.v1.json":
        return {
            "status": "loaded",
            "baseline_schema_version": left.get("schema_version"),
            "candidate_schema_version": right.get("schema_version"),
            "baseline": left,
            "candidate": right,
            "unexpected": False,
        }
    left_hash = digest_bytes(stable_json_bytes(left))
    right_hash = digest_bytes(stable_json_bytes(right))
    return {
        "status": "equal" if left_hash == right_hash else "different",
        "baseline_sha256": left_hash,
        "candidate_sha256": right_hash,
        "unexpected": left_hash != right_hash,
    }


def fixture_dir(root: Path, fixture: str) -> Path:
    return root / fixture


def compare_fixture(baseline_root: Path, candidate_root: Path, fixture: str) -> dict[str, Any]:
    baseline_dir = fixture_dir(baseline_root, fixture)
    candidate_dir = fixture_dir(candidate_root, fixture)
    if not baseline_dir.is_dir() or not candidate_dir.is_dir():
        missing = {
            "baseline_present": baseline_dir.is_dir(),
            "candidate_present": candidate_dir.is_dir(),
        }
        return {
            "geometry": {"status": "missing_fixture", **missing, "unexpected": True},
            "ocr": {"status": "missing_fixture", **missing, "unexpected": True},
            "api": {"status": "missing_fixture", **missing, "unexpected": True},
        }

    json_results: dict[str, dict[str, Any]] = {}
    loaded: dict[str, tuple[dict[str, Any] | None, dict[str, Any] | None]] = {}
    for filename in JSON_FILES:
        item = compare_json_file(baseline_dir / filename, candidate_dir / filename, filename)
        json_results[filename] = item
        if filename == "result.v1.json" and item.get("status") == "loaded":
            loaded[filename] = (item["baseline"], item["candidate"])

    left_v1, right_v1 = loaded.get("result.v1.json", (None, None))
    if left_v1 is None or right_v1 is None:
        geometry = {"status": "missing_or_invalid_v1", "unexpected": True}
        ocr = {"status": "missing_or_invalid_v1", "unexpected": True}
    else:
        left_geometry, right_geometry = extract_geometry(left_v1), extract_geometry(right_v1)
        geometry_differences = flatten_diff(left_geometry, right_geometry)
        geometry = {
            "status": "equal" if not geometry_differences else "different",
            "baseline": left_geometry,
            "candidate": right_geometry,
            "differences": geometry_differences,
            "unexpected": bool(geometry_differences),
        }
        left_ocr, right_ocr = extract_ocr(left_v1), extract_ocr(right_v1)
        ocr_differences = flatten_diff(left_ocr, right_ocr)
        ocr = {
            "status": "equal" if not ocr_differences else "different",
            "baseline": left_ocr,
            "candidate": right_ocr,
            "differences": ocr_differences,
            "unexpected": bool(ocr_differences),
        }

    export_results = {
        "result.docx": compare_optional_file(baseline_dir / "result.docx", candidate_dir / "result.docx", "docx"),
        "result.xlsx": compare_optional_file(baseline_dir / "result.xlsx", candidate_dir / "result.xlsx", "xlsx"),
    }
    legacy_json = json_results.get("result.json", {})
    legacy_equal = legacy_json.get("status") in ("equal", "not_present_both")
    left_document = left_v1.get("document") if isinstance(left_v1, dict) else None
    right_document = right_v1.get("document") if isinstance(right_v1, dict) else None
    left_document_hash = None if not isinstance(left_document, dict) else digest_bytes(stable_json_bytes(left_document))
    right_document_hash = None if not isinstance(right_document, dict) else digest_bytes(stable_json_bytes(right_document))
    legacy_projection = {"status": "not_present_both"}
    if legacy_json.get("status") == "equal":
        try:
            left_legacy_doc = load_json(baseline_dir / "result.json")
            right_legacy_doc = load_json(candidate_dir / "result.json")
            left_cross_equal = isinstance(left_document, dict) and stable_json_bytes(left_legacy_doc) == stable_json_bytes(left_document)
            right_cross_equal = isinstance(right_document, dict) and stable_json_bytes(right_legacy_doc) == stable_json_bytes(right_document)
            legacy_projection = {
                "status": "equal" if left_cross_equal and right_cross_equal else "different",
                "baseline_matches_v1_document": left_cross_equal,
                "candidate_matches_v1_document": right_cross_equal,
            }
        except (OSError, json.JSONDecodeError, ValueError):
            legacy_projection = {"status": "invalid_json"}
    elif legacy_json.get("status") != "not_present_both":
        legacy_projection = {"status": legacy_json.get("status", "missing")}
    schema_compatible = (
        left_v1 is not None
        and right_v1 is not None
        and isinstance(left_document, dict)
        and isinstance(right_document, dict)
        and left_v1.get("schema_version") == 1
        and right_v1.get("schema_version") == 1
    )
    legacy_projection_equal = (
        left_v1 is not None
        and right_v1 is not None
        and left_document_hash == right_document_hash
        and legacy_projection.get("status") in ("equal", "not_present_both")
    )
    exports_equal = not any(item.get("unexpected") for item in export_results.values())
    compatible = legacy_equal and schema_compatible and legacy_projection_equal and exports_equal
    api = {
        "status": "compatible" if compatible else "different",
        "schema_v1": {
            "baseline": None if left_v1 is None else left_v1.get("schema_version"),
            "candidate": None if right_v1 is None else right_v1.get("schema_version"),
        },
        "legacy_json": legacy_json,
        "legacy_document_projection": {
            "status": "equal" if left_v1 is not None and right_v1 is not None and left_document_hash == right_document_hash else "different_or_missing",
            "baseline_sha256": left_document_hash,
            "candidate_sha256": right_document_hash,
            "cross_export_equality": legacy_projection,
        },
        "exports": export_results,
        "unexpected": (
            not legacy_equal
            or left_v1 is None
            or right_v1 is None
            or not schema_compatible
            or not legacy_projection_equal
            or legacy_projection.get("status") not in ("equal", "not_present_both")
            or any(x.get("unexpected") for x in export_results.values())
        ),
    }
    return {"geometry": geometry, "ocr": ocr, "api": api}


def write_report(path: Path, category: str, fixtures: dict[str, Any]) -> bool:
    unexpected = any(
        fixture.get(category, {}).get("unexpected", False)
        for fixture in fixtures.values()
    )
    report = {
        "report": category,
        "status": "different" if unexpected else "equal",
        "fixtures": {
            fixture: values.get(category, {})
            for fixture, values in fixtures.items()
        },
    }
    path.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    return unexpected


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", required=True, type=Path)
    parser.add_argument("--candidate", required=True, type=Path)
    parser.add_argument("--report-dir", required=True, type=Path)
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    if not args.baseline.is_dir() or not args.candidate.is_dir():
        print('{"status":"error","reason":"input_directory_missing"}')
        return 2
    try:
        args.report_dir.mkdir(parents=True, exist_ok=True)
        fixtures = {
            fixture: compare_fixture(args.baseline, args.candidate, fixture)
            for fixture in FIXTURES
        }
        changed = False
        changed |= write_report(args.report_dir / "GEOMETRY_REGRESSION.json", "geometry", fixtures)
        changed |= write_report(args.report_dir / "OCR_TEXT_REGRESSION.json", "ocr", fixtures)
        changed |= write_report(args.report_dir / "API_EXPORT_COMPATIBILITY.json", "api", fixtures)
    except Exception:
        print('{"status":"error","reason":"comparison_or_report_failed"}')
        return 2
    status = "different" if changed else "equal"
    print(json.dumps({"status": status, "fixtures": list(FIXTURES)}, separators=(",", ":")))
    return 1 if changed else 0


if __name__ == "__main__":
    sys.exit(main())
