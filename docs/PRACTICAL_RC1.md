# Practical Windows RC1 review order

This integration branch combines the portable infrastructure, its inventory
fixes, the PDFium notice packet and the positioned-text corrections. It does
not authorize a binary release. The source review is separate from licensing
and clean-VM acceptance.

1. Review [inventory PR #4](https://github.com/paulgreenbox-beep/rust-document-ocr/pull/4)
   against [portable PR #2](https://github.com/paulgreenbox-beep/rust-document-ocr/pull/2).
2. Review the practical changes and synthetic fixtures in
   [source PR #5](https://github.com/paulgreenbox-beep/rust-document-ocr/pull/5).
3. Review [PDFium notices PR #3](https://github.com/paulgreenbox-beep/rust-document-ocr/pull/3)
   and the [PDFium release gate](pdfium-7881/RELEASE_GATE.md).
4. Reconcile the reviewed branches before the owner merges a final tree. The
   integration branch is evidence for the combination, not a replacement for
   review of the dependent PRs. If the final tree changes, bind a new build and
   its tests to the actual resulting commit.

Keep separate records for source CI, the locked Windows EXE build, behavioral
regression, license review and clean-Windows validation. The existing portable
packager must continue to reject a distributable ZIP without the required
evidence and approvals. No private inputs, model files or local build receipts
belong in a public review branch.

## Current owner decision for RC1

| Environment check | Status |
| --- | --- |
| Windows Sandbox | **NOT RUN — DEFERRED BY OWNER** |
| Separate clean Windows x64 installation/VM | **NOT RUN — DEFERRED BY OWNER** |

Continue source review, local package documentation and license review without
these checks. The existing developer-host smoke with controlled PATH does not
qualify either environment. Deferral is not a failure, a pass, or permission
to bypass the distribution gate. Do not create a clean-environment receipt or
enable ZIP/public binary publication on that basis.

The already tested EXE and local package remain bound to build source
`1af284f2cbf1201972b02060ace287b27150382b`. Later documentation-only changes do
not change that build identity. No native rebuild or OCR rerun is required just
to record this decision; the historical build and test receipts stay intact.

The [current review status](compliance/RC1_REVIEW_STATUS.md) separates verified
component identities from open redistribution decisions and records findings
for the source and ZIP review. It does not amend the tested payload or approve
publication.
