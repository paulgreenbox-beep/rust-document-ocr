# heic-rs — recipient notices for the pure-HEIC candidate

Review: 2026-11-09. **Notice materials verified; packet ready for inclusion
review, not approved for distribution.** Keep the four accompanying files
below with this index in the recipient's `licenses/` directory. The index
does not replace the license texts or NOTICE. This review did not repack or
publish the candidate.

This program includes a local derivative of **heic-rs**, version
**0.1.4-ocr.1**, declared **MIT OR Apache-2.0**. Upstream copyright:
**Copyright (c) 2026 Thomas Braun** (retained in the MIT text).
The derivative is not an upstream release. Both offered license texts, the
original upstream NOTICE and the local modification record are retained.
The publisher should record which applicable option it relies on and preserve
the corresponding copyright, license, attribution and change notices; this
index does not make that selection for the publisher.

## Required accompanying files (exact basenames)

| File beside this index | Bytes | SHA-256 |
| --- | ---: | --- |
| `crate-heic-rs-0.1.4-ocr.1-LICENSE-MIT` | 1,069 | `b1919069bdd26147c90fcb6795feb340f3324c224438e17132584bef17543a22` |
| `crate-heic-rs-0.1.4-ocr.1-LICENSE-APACHE` | 10,847 | `a60eea817514531668d7e00765731449fe14d059d3249e0bc93b36de45f759f2` |
| `crate-heic-rs-0.1.4-ocr.1-NOTICE` | 1,021 | `c16bd7c1ab22165a83295d602a0fec289035b13fbc7345b41a020c339020b96a` |
| `crate-heic-rs-0.1.4-ocr.1-PATCH_NOTES.md` | 7,049 | `9f1f6cf91b1e3f55466b0c56b94e61f3f891bdaee43ae8466c1431dfdd9285b3` |

The first three files match the cached [published heic-rs 0.1.4 archive](https://static.crates.io/crates/heic-rs/heic-rs-0.1.4.crate)
byte-for-byte. Its SHA-256 is
`8883c3bfa6b4ff19a40e52a6dc051c5612cbe21bf408fc6d4875e8dfd777cb66`,
and its embedded VCS record identifies commit
`999470f514c8e9a1a7c37ea44ba70cc32b1308fe`. All four candidate files also
match the [pinned derivative source](https://github.com/paulgreenbox-beep/rust-document-ocr/tree/3185a5f5f1c79a5036b2a0c6d63e6334c4f66f26/tools/heic-rs-evaluation/vendor/heic-rs)
and candidate release-manifest records. No original decoder license text
was missing from the local candidate. The newly supplied index binds their
recipient names and identities without duplicating or rewriting the originals.

The retained link receipt binds derivative rlib SHA-256
`0b34396b1aea5c5d7ae366eff23f4b73467e5b0dffc1485da4b469955c261567`
to the pure candidate EXE SHA-256
`c6cddc7516d5e44b225cde2fabb35e534016e05a7c900774b087019e538dfc23`.
This is the scope of the notice check; it is not an assertion about legacy
`native`, RC1, or an arbitrary build.

## Modification and upstream-asset scope

Local changes dated **2026-11-07** add bounded coded-picture/aggregate raw
HEVC preflight and resource-limit reporting, and correct the `imir`
reflection-axis interpretation. The packaged `PATCH_NOTES.md` identifies
the modified source files and details the changes; it does not alter the
original upstream attribution.

The retained upstream NOTICE also describes its references to other
decoders, branded artwork and IBM Plex fonts. That description is kept intact.
The reviewed derivative tree has no tracked upstream `assets/` subtree. None
of the eight asset files in the published archive matches a candidate manifest
entry by basename or SHA-256. This is a listed-payload check, not analysis of
all possible embedded data. The NOTICE does not give this publisher permission
to add upstream brand/font assets. If they are later included, review their
own terms and applicable notices separately.

## Patent boundary

The upstream NOTICE disclaims a grant to the HEVC standard. The [Apache-2.0
text, section 3](https://www.apache.org/licenses/LICENSE-2.0) concerns the
licensors' qualifying patent claims, not a blanket third-party HEVC patent
license. MIT/Apache code notices and the absence of legacy libheif are not
patent clearance. Patent questions require a separate decision by the
responsible reviewer before public distribution; private legal findings do
not belong in the recipient notice packet.
