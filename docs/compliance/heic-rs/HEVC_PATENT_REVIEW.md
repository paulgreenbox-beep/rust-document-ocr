# HEVC patent-review worksheet

**UNSIGNED — PATENT DECISION OPEN.** This is an intake and evidence checklist,
not a patent opinion, pool-license recommendation, or permission to distribute.
Keep confidential agreements, account details and legal advice outside Git.

## Product facts for review

The candidate is Windows x64 desktop OCR with bundled software HEIF/HEIC
decoding through modified `heic-rs 0.1.4-ocr.1`. The examined adapter
probes and decodes input still images locally; it does not expose an encoder
or a video-streaming service. The container and codec are distinct:
[ISO/IEC 23008-12:2025](https://www.iso.org/standard/89035.html) describes the
image-file format; [ITU-T H.265](https://www.itu.int/rec/T-REC-H.265)
specifies the codec. These links identify standards, not a promise of support
for every edition, feature, or patent claim. Use the exact EXE and derivative
identities in [recipient notices](RECIPIENT_NOTICES.md).

The publisher must confirm that this factual use, actual supported profiles
and release contents match the proposed distribution. Do not infer exemption
from local operation, still-image use, decode-only behavior, free downloads,
MIT/Apache licensing, absence of libheif, or an OS/device vendor's license.

## Questions to resolve

| Question | Written decision/evidence needed |
| --- | --- |
| Who distributes, and where? | Exact legal entity/affiliates, release host and channel, intended countries and end users; jurisdictions in scope for offering, downloading and use, and any enforceable limitations |
| What acts and product? | Exact manifest and EXE hashes, documented codec profiles/functions actually shipped; local still-image decoding only or also service, streaming, encode, OEM, downstream and/or commercial uses; which acts matter under applicable law |
| How is it distributed? | Free/paid/licensed/bundled, versions, recipients, expected units/downloads or other measures required by the reviewed terms; do not assume a distribution tier applies |
| What claims/rights are relevant? | Review applicable in-force claims/territories, claim-to-implementation mapping, expirations/ownership and pertinent exceptions; record date/evidence rather than treating standard declarations or a single pool list as exhaustive |
| Is there existing coverage? | If relying on a pool, bilateral, supplier, device or other grant, identify signed grant, licensor, licensee, product/territory/activity scope, term and downstream treatment; expressly check coverage for this independently bundled decoder |
| What remains to do? | Required permission, reporting, royalty, notice, sourcing, geographic or distribution restrictions and who owns each obligation; re-review when product scope or terms change |
| Release disposition | **APPROVED FOR EXACT SCOPE / HOLD / NOT APPLICABLE WITH WRITTEN BASIS**; reviewer, legal/publisher approval, date and private evidence reference. Unanswered material questions remain HOLD |

Primary leads for counsel, not coverage conclusions:
[ITU-T IPR and declaration resources](https://www.itu.int/en/ITU-T/ipr/Pages/default.aspx),
[Access Advance HEVC pool product scope](https://accessadvance.com/topic-what-do-we-license/),
its [official FAQ](https://accessadvance.com/faq/), and the
[VCL Advance program page](https://accessadvance.com/licensing-programs/vcl-advance/).
The administrators' descriptions are not the publisher's agreement; check
current terms, potentially overlapping rights and non-pool claimants. No
provider has been contacted or authorized to act on behalf of the publisher.

The [code-notice check](RECIPIENT_NOTICES.md) is separate. This worksheet
does not authorize dropping codec functionality, modifying the candidate,
changing package gates or releasing it.
