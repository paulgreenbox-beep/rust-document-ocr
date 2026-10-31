# vcpkg installed-state inventory tooling

This tooling gathers evidence, not binary-distribution approval. All output
remains `distribution_status: blocked`. See the
[historical erratum](VCPKG_INVENTORY_ERRATUM_2026-10-31.md).

## Contract

- Core identity is `(Package, Architecture)` where Architecture is the triplet.
  Core and feature records are parsed separately; features never replace core
  Version, Port-Version or ABI. Duplicate cores, missing required core records,
  mismatched `.list` versions and orphan lists fail explicitly.
- Original Version and optional Port-Version remain separate fields. The display
  version adds `#port-version` only for a nonzero port version. Actual `.list`
  names may contain the base version without that suffix.
- Library identity is its full installed-relative path. Release and debug
  libraries with the same basename are separate records. Unsafe paths, ambiguous
  ownership, input/output overlap and unlisted archives are rejected.
- ABI receipt bytes are SHA-256 checked against that package's status ABI.
  SPDX package/version/ABI and available recorded file hashes are compared.
  Missing evidence is `UNAVAILABLE` / `NOT CHECKED`, not a successful comparison.
  An absent package-side `BUILD_INFO` is explicitly `NOT CHECKED`.
- `scripts/vcpkg_inventory.py` is filesystem-free. Its `validate_metadata` helper
  validates supplied receipt bytes and supplied file hashes; it does not measure
  libraries itself. The normal collector hashes installed files with per-file
  stability checks and rechecks metadata/membership, but is not an atomic
  filesystem snapshot. Do not concurrently modify the installation.

### Aggregate compatibility and explicit scope change

The release aggregate algorithm is unchanged: sort archives by case-sensitive
basename; join `basename:UPPERCASE_SHA256` records with LF and **no final LF**;
SHA-256 that UTF-8 text and uppercase the result. Case-insensitive duplicate
basenames that this format cannot safely identify are rejected. Debug archives
are stored separately and do not enter this release aggregate.

The collection **population is now explicit installed state**: all selected-
triplet core packages and their list-owned release/debug archives are retained.
Previously the collector could filter by saved Cargo build-script link requests
and could search directories by basename. Those requests are incomplete evidence
of final linkage, so they now annotate `rustc_link_kinds` only. This is recorded
in `evidence.vcpkg_inventory_scope`, not silently presented as executable closure.
An installation can therefore produce a different aggregate input set even
though the byte-level formula is unchanged. In the preserved 2026-10-31 capture,
the exact release path sets and all 24 historical hashes match; that measured
case is not a universal equality assertion for future installations.

## Source-only tests

Use existing Python 3.11 or later; no pip framework, native libraries, Cargo,
paid APIs, local capture or customer files are needed:

```text
python scripts/test_portable_inventory.py -v
python -c "import ast,pathlib; [ast.parse(p.read_text(encoding='utf-8'), filename=str(p)) for p in pathlib.Path('scripts').glob('*.py')]"
git diff --check
```

Tests generate tiny text-only synthetic package trees in owned temporary
directories, including fake `.lib`-named text files, and clean up only those
fixtures. They do not open installed vcpkg payloads. The Python-only CI job
executes these gates; the existing Windows source checks remain enabled.

## Replay an already saved capture

This path reads only the capture, never the vcpkg installation. Verify the
manifest and index digests from a separately retained trusted receipt:

```text
python scripts/verify_vcpkg_capture.py --capture <capture-directory> --expected-manifest-sha256 <sha256> --expected-index-sha256 <sha256>
```

The report distinguishes saved-file hash verification, metadata-to-metadata
comparisons and comparisons with **recorded** payload hashes. It reports
`real_payload_files_rehashed: 0`; no historic EXE link receipt is invented.
The original `.local.json`, raw receipts and old evidence copies stay private
and unmodified.

## Create a public derivative of this observation

```text
python scripts/public_vcpkg_inventory.py --input <saved-local-manifest> --output <new-public-json>
```

This deliberately observation-specific CLI verifies the exact local source
digest before parsing. Its pure `public_projection` API also accepts synthetic
inputs for offline tests. It constructs a fixed allowlist, checks archive paths,
sizes and SHA-256 against the owning package's recorded file set, recalculates
comparisons and rejects inconsistent source assertions. Unknown private fields
are not forwarded, including error strings and nested payload metadata. Safe
relative paths and Windows-reserved-name checks prevent filesystem ambiguity.

The output has a separate checksum sidecar. It explicitly says
`derived_from_local_observation: true`, `historical_link_receipt: false`, and
`distribution_status: blocked`. Neither the projection nor its checker reruns
the physical-file measurement. Original SPDX license strings remain exactly
source metadata. Review the allowlisted document before adding it to Git; never
add an entire local capture or disable the repository's private-report ignores.

No collector result clears LGPL source/relink, Microsoft redistribution rights,
retained static-code closure, PDFium license gates, or clean-Windows acceptance.
No runtime, model, dependency, OCR algorithm, ZIP or release changes belong to
this tooling fix.
