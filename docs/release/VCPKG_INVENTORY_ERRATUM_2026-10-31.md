# Erratum: vcpkg inventory metadata (2026-10-31)

Status: historical metadata correction only. The Windows portable distribution status remains `blocked`.

This erratum clarifies the historical candidate inventory without rewriting its manifest or evidence. It concerns the collector at commit `e031efd17fa7fe8dcfd2240535692ad5ac7d9efd`, file `scripts/collect-portable-inventory.py` (SHA-256 `afc4ed4a55b742fcc53d05260c367906365cf51f9655f94174c25877c9d99ae9`), and the associated `reports/WINDOWS_PORTABLE_CANDIDATE_MANIFEST.json` (SHA-256 `8803f1eb4a5cd50e96707d19f78e93d43114cca373fa52f65bfc155fc265b4db`). The artifact hashes were checked against retained evidence copies. The fresh local observation manifest is identified by SHA-256 `dedb8078c29714c0b3a56ca83ceb1f258479848319d9a47b5cca204eb87c811a`; it is cited by digest only, not reproduced here.

## Findings

1. **The nested `installed_status` association was wrong.** In the historical collector, `status` was assigned while iterating installed `.list` files, then the later archive-emission loop reused that function-local variable instead of looking up the status for the current archive's port. The final list entry was zstd, so zstd's nested status was emitted for other components. The retained comparison reports 17 of 18 historical nested status objects discrepant; zstd is the sole object that matches its own fresh status. This makes those historical nested fields unreliable for the affected components; it does not by itself invalidate their separate top-level version fields.

2. **All 18 top-level component versions match.** The retained comparison reports that every historical top-level version agrees with the corresponding fresh observation. This is a separate comparison from the defective nested `installed_status` objects.

3. **All 24 recorded historical release hashes match current files in the prior measurement.** The saved 2026-10-31 two-pass local report records a match for each of the 24 historical release SHA-256 values against the then-current installed files. This erratum does not repeat or independently rerun that file-hash comparison. The result corroborates byte equality in that measurement; it does not establish when, how, or whether those files were linked into the historical executable.

4. **Historical executable linkage and archive-member retention remain unknown.** The fresh inventory is a bounded observation of installed files, not a recovered historical linker receipt. It does not prove that every member of every archive was retained in the executable, or identify which members contributed to it.

## Remaining release limits

The old manifest and collector remain historical evidence and are not edited retroactively. This note makes no claim that a separate collector source change has passed review or acceptance. The new inventory does not close the LGPL source/relink obligations, MSVC redistribution entitlement, full statically linked member-closure review, or clean-Windows-VM acceptance. These gates remain open; distribution remains `blocked`.

## Existing fix and this follow-up

The current portable base already contains the minimal per-port lookup fix in
[commit 0d0f092](https://github.com/paulgreenbox-beep/rust-document-ocr/commit/0d0f092eececbf051d7fe09196444c8008512aaa).
This follow-up starts from `d82389c197226f894c85918224358b9ad2ca9b1c` on
`release/windows-portable-v1`, not from `main` and not from the PDFium notices
branch. It does not claim that the historical stale-variable defect remained in
that base. Its additions validate `(package, triplet)` identity, separate core
and feature records, reject conflicting/missing core records and ambiguous
ownership, preserve both release and debug paths, and check supplied ABI/SPDX
receipt bytes. Missing evidence stays `UNAVAILABLE` or `NOT CHECKED`.

The two historical-defect tests fail against the retained `e031efd` collector
and pass with this implementation. The complete Python suite passes locally
(42 tests). A dedicated Python-only CI job runs independently of Rust/native
runtime; the existing Windows source checks are retained. The resulting PR's
checks, rather than this document, provide the current commit-bound CI result.

For the saved final capture, the corrected helper replays 80 checksum-indexed
files, including real saved status, `.list`, ABI receipts and SPDX JSON. It
recomputes the 18 version matches, 24 release hash comparisons, 18 exact release
archive-set matches and 17 nested-status discrepancies. All 18 ABI and SPDX
identity checks and 786 comparisons against recorded payload hashes pass;
18 absent `BUILD_INFO` entries remain `NOT CHECKED`. This replay **does not
reopen the 822 installed payload files**. Their physical-file measurement is
the earlier bounded two-pass observation, not a new scan or atomic snapshot.

## Public derivative and reproducibility

- [Allowlisted public inventory](../../reports/vcpkg-inventory-refresh-20261031/VCPKG_INVENTORY_PUBLIC.json)
- [Its own checksum](../../reports/vcpkg-inventory-refresh-20261031/VCPKG_INVENTORY_PUBLIC.json.sha256)
- [Tooling contract and commands](VCPKG_INVENTORY_TOOLING.md)

Public projection SHA-256:
`23ba712cea23608caa70dccb9f62a1dd06173e987cc83f1a452623485f7553ec`.
This is the hash of the public derivative, **not** the hash of its private local
source. The public document is constructed from an allowlist; local paths,
stdout, machine identity and private free text are not copied. License strings
including `LicenseRef-vcpkg-null` remain source metadata, not guessed licenses
or grants of distribution permission.
