"""Summarize full CLI A/B without exposing input paths or recognized text.

Raw JSON/DOCX/XLSX and stdout/stderr remain in the caller-owned run directory.
Uses the existing source release's OOXML comparison, not byte equality of ZIPs.
"""
from __future__ import annotations

import argparse
import collections
import copy
import hashlib
import importlib.util
import json
import re
import xml.etree.ElementTree as ET
from pathlib import Path


PROJECT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("audit", PROJECT / "scripts/compare_audit_results.py")
audit = importlib.util.module_from_spec(spec)
assert spec.loader
spec.loader.exec_module(audit)


def tokens(text: str) -> collections.Counter:
    return collections.Counter(re.findall(r"\w+", text.casefold(), re.UNICODE))


def metric(text: str, reference: str) -> dict:
    actual, expected = tokens(text), tokens(reference)
    matched = sum((actual & expected).values())
    precision = matched / sum(actual.values()) if actual else 0.0
    recall = matched / sum(expected.values()) if expected else 0.0
    return {"precision": precision, "recall": recall,
            "f1": 2 * precision * recall / (precision + recall) if precision + recall else 0.0}


def non_timing(value: dict) -> dict:
    value = copy.deepcopy(value)
    for page in value.get("page_processing", []):
        page.pop("timings", None)
    return value


def summarize(root: Path, run: dict) -> dict:
    record = {key: run[key] for key in ("exit_code", "timeout", "elapsed_ms")}
    record["capture_incomplete"] = run.get("capture_incomplete", False)
    path = root / run["output"]
    if run["exit_code"] != 0 or run["timeout"]:
        record["output_published"] = path.is_dir()
        return record
    value = json.loads((path / "result.v1.json").read_text(encoding="utf-8"))
    assert value["schema_version"] == 1
    pages = value["document"]["pages"]
    text = "\n".join(page.get("text", "") for page in pages)
    grids = [grid for page in value["tables"] for grid in page["grids"]]
    cells = [cell for grid in grids for cell in grid["cells"]]
    record.update(pages=len(pages), characters=sum(len(p.get("text", "")) for p in pages),
                  words=sum(tokens(text).values()), text_sha256=hashlib.sha256(text.encode()).hexdigest(),
                  grids=len(grids), cells=len(cells), filled_cells=sum(bool(c["text"].strip()) for c in cells),
                  modes=[p["processing_mode"] for p in value["page_processing"]],
                  confidence=[p.get("ocr_confidence") for p in value["page_processing"]],
                  word_assignment=[p.get("word_assignment") for p in value["page_processing"]],
                  warnings=len(value["warnings"]),
                  artifact_sizes={a["name"]: a["size"] for a in run["artifacts"]})
    if run["lane"] == "text":
        svg = PROJECT / "tools/heic-rs-evaluation/fixtures/ocr-public" / (run["case"] + "-fixture.svg")
        reference = " ".join(t for el in ET.parse(svg).iter() if el.tag.endswith("}text")
                             for t in el.itertext())
        record["synthetic_source_token_metrics"] = metric(text, reference)
    return record


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("run_root", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists():
        raise SystemExit("Refuse to overwrite a comparison report")
    receipt = json.loads((args.run_root / "runs.json").read_text(encoding="utf-8-sig"))
    grouped = {}
    for run in receipt["runs"]:
        variants = grouped.setdefault(run["case"], {})
        if run["variant"] in variants or run["variant"] not in {"native", "heic_rs"}:
            raise ValueError("Duplicate or unknown A/B variant")
        variants[run["variant"]] = run
    results = []
    for case, variants in grouped.items():
        baseline, candidate = variants["native"], variants["heic_rs"]
        for key in ("input", "input_sha256", "lane"):
            if baseline[key] != candidate[key]:
                raise ValueError(f"A/B input mismatch: {case}/{key}")
        row = {"case": case, "lane": baseline["lane"], "input_sha256": baseline["input_sha256"],
               "native": summarize(args.run_root, baseline), "heic_rs": summarize(args.run_root, candidate)}
        if baseline["exit_code"] == candidate["exit_code"] == 0 and not (
            baseline["timeout"] or candidate["timeout"]
            or baseline.get("capture_incomplete", False) or candidate.get("capture_incomplete", False)
        ):
            left, right = (args.run_root / run["output"] for run in (baseline, candidate))
            a = json.loads((left / "result.v1.json").read_text(encoding="utf-8"))
            b = json.loads((right / "result.v1.json").read_text(encoding="utf-8"))
            differences = audit.flatten_diff(non_timing(a), non_timing(b))
            row["comparison"] = {
                "identical_input_verified": True,
                "exact_non_timing_json": not differences,
                "legacy_result_json_equal": audit.load_json(left / "result.json") == audit.load_json(right / "result.json"),
                "different_fields": [item["field"] for item in differences],
                "geometry_equal": audit.extract_geometry(a) == audit.extract_geometry(b),
                "ocr_fingerprints_equal": audit.extract_ocr(a) == audit.extract_ocr(b),
                "docx_document_xml_equal": audit.docx_fingerprint(left / "result.docx") == audit.docx_fingerprint(right / "result.docx"),
                "xlsx_workbook_cells_equal": audit.xlsx_fingerprint(left / "result.xlsx") == audit.xlsx_fingerprint(right / "result.xlsx"),
            }
            row["status"] = "EXACT_NON_TIMING_MATCH" if not differences else "DIFFERENCES_RECORDED"
        else:
            if baseline.get("capture_incomplete", False) or candidate.get("capture_incomplete", False):
                row["status"] = "CAPTURE_INCOMPLETE"
            else:
                row["status"] = "CANDIDATE_ONLY_SUCCESS" if candidate["exit_code"] == 0 and not candidate["timeout"] else "DECODE_OR_PIPELINE_FAILURE"
        results.append(row)
        print(f"{case}: {row['status']}; native chars={row['native'].get('characters')}, heic-rs chars={row['heic_rs'].get('characters')}")
    report = {"schema_version": 1,
        "scope": "Research pipeline A/B; timings illustrative, not distribution or performance qualification",
        "build_profile": receipt.get("build_profile", "not-recorded"),
              "token_metric": "Casefold Unicode word multiset against upstream synthetic SVG source; no OCR-confusion substitutions",
              "binary_identity": receipt["binaries"], "settings": receipt["settings"], "cases": results}
    args.output.write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
