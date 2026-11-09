# Windows release decisions — unsigned

For the pure-HEIC Windows x64 candidate in the [licensing closure packet](docs/compliance/WINDOWS_RELEASE_LICENSING_CLOSURE.md), existing manifest SHA-256 `7f2381924d4521feef2b4bebc0b69d9d95d99fb03e0efbed254c64fd390d6801`. This is a decision checklist, not a redistribution grant. All approvals below remain pending. Record private evidence by reference; do not upload agreements, training material, local paths, or binary artifacts.

- Publisher/legal entity: __________
- Decision date and authorized signatory: __________
- Intended distribution channel, territories, and recipients: __________
- Final assembled candidate manifest SHA-256 (when available): __________
- Private approval record reference: __________

| Decision | Exact confirmation needed | Prepared evidence |
| --- | --- | --- |
| PDFium 7881 | For DLL SHA-256 `79d4676b656cfb1abcea88f9ade3b4b0826c5200382db5f4ec72a636c598c118`, record whether the notice scope is supported by adequate evidence for static/generated/bundled content. Identify the historical gaps resolved and the evidence or lawful alternative relied on. If insufficient, keep blocked; request an independently documented artifact, not an automatic runtime replacement. | [Recipient tree and limits](docs/pdfium-7881/RECIPIENT_PACKET.md) |
| EG79 | Complete the authority confirmation for the exact weights, inherited Apache-2.0 terms, training inputs, and output rights; keep both model notices. An unresolved rights record does not authorize silently dropping or replacing the model in this candidate. | [EG79 unsigned confirmation](docs/compliance/eg79/OWNER_CONFIRMATION.md) |
| Microsoft CRT | Identify the qualifying licensee/publisher, agreement and clause for each of the three byte-identified unmodified x64 DLLs; confirm channel, applicable downstream terms, and servicing. Resolve separately applicable statically incorporated Microsoft/SDK materials. | [CRT unsigned confirmation](docs/compliance/crt/PUBLISHER_CONFIRMATION.md) |
| Modified heic-rs | Record the applicable MIT/Apache-2.0 option and local patch authority. Verify the actual recipient index and four companion texts after assembly. | [Decoder recipient notices](docs/compliance/heic-rs/RECIPIENT_NOTICES.md) |
| HEVC patents | Have a qualified reviewer record whether the proposed distribution/use needs any patent grant or other condition, its scope and territories, who holds it and what further action is required. Leave distribution blocked if unresolved. | [HEVC patent worksheet](docs/compliance/heic-rs/HEVC_PATENT_REVIEW.md) |
| Other dependencies and own Rust modules | Confirm authority to distribute the listed project source/compiled form under MIT, including technical stamp and table modules; record inbound licenses or contributor authorization where applicable. Record license alternatives used and resolve native/compiler evidence gaps. | [Dependency matrix](docs/compliance/windows-dependencies/COMPONENT_MATRIX.json), [project-code provenance](docs/compliance/windows-dependencies/PROJECT_CODE_PROVENANCE.md) |
| Final packaging | On the exact final manifest, verify all required files, hashes, license bindings, readable paths and credits. Retain fail-closed gates. Record reviewer/date and the private evidence references for every cleared item; no draft PR or CI status substitutes for approval. | [Future assembly checklist](docs/compliance/WINDOWS_RELEASE_LICENSING_CLOSURE.md#final-assembly-and-independent-gates) |

**Separate environment status:** Windows Sandbox and separate clean Windows x64 are both **NOT RUN — DEFERRED BY OWNER**. Do not reinterpret this as a pass/fail outcome, create a substitute receipt, or remove a packaging gate. **WINDOWS PORTABLE RELEASE — BLOCKED: LICENSING** until the applicable rights and closure decisions are documented. Independent technical gates must also be resolved before a binary release.
