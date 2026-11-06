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
