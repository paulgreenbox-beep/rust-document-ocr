# EG79 release notice packet

**NOTICE FILES READY FOR STAGING; WEIGHT REDISTRIBUTION RIGHTS OPEN.** This
packet supplies documentation only and does not modify any existing package.

## Evidence and correction

- The exact pure-HEIC local candidate manifest (`release-manifest.json`
  SHA-256 `7f2381924d4521feef2b4bebc0b69d9d95d99fb03e0efbed254c64fd390d6801`)
  declares component `technical-cell-model-eg79` with `notices: []` and
  `distribution_status: blocked`. A fresh read-only SHA-256 and size check of
  its actual model file matched [NOTICE.txt](NOTICE.txt). The existing package
  license `licenses/tessdata-Apache-2.0.txt` is bound to **tessdata_fast**, not
  EG79; it is not a model-specific modification notice.
- The retained training manifest binds the same derived model SHA-256 to the
  English `tessdata_best` base SHA-256 and Tesseract 5.5.2. The retained dataset
  manifest also matches its recorded hash; that verifies byte identity, not
  rights in its records. No training examples or labels were disclosed.
- The [prior byte-level base match](../RC1_REVIEW_STATUS.md#source-references-and-evidence-boundaries)
  identifies equivalent source bytes, not the historical download route. The
  [pinned upstream tree](https://api.github.com/repos/tesseract-ocr/tessdata_best/git/trees/e2aad9b983032bb1beff9133104a67cdbb87ca4d?recursive=1)
  re-confirmed the base blob and license blob; the response was not truncated
  and contained no separate upstream NOTICE or COPYRIGHT file. The
  [pinned README](https://github.com/tesseract-ocr/tessdata_best/blob/e2aad9b983032bb1beff9133104a67cdbb87ca4d/README.md)
  declares Apache-2.0 for the data. This does not infer absent metadata inside
  model files or any separate historical material that is not in that tree.
- [LICENSE.tessdata_best.txt](LICENSE.tessdata_best.txt) is an exact copy of the
  [pinned license](https://github.com/tesseract-ocr/tessdata_best/blob/e2aad9b983032bb1beff9133104a67cdbb87ca4d/LICENSE):
  10,173 bytes, Git blob `d9a10c0d8e868ebf8da0b3dc95bb0be634c34bfe`, SHA-256
  `a6cba85bc92e0cff7a450b1d873c0eaa2e9fc96bf472df0247a26bec77bf3ff9`.
  This is the upstream file, not a replacement or a license declaration for
  the unresolved local contributions.

## Recipient placement and final approval

For a future authorized package, stage `NOTICE.txt` and
`LICENSE.tessdata_best.txt` together under `licenses/eg79/`, reference both
from `technical-cell-model-eg79` in the **new** manifest, and link the notice
from the recipient's license index so the change to the named binary model is
prominent. Verify the final paths, hashes and exact model identity. Keep the
standard tessdata_fast notices. The companion notice preserves the model bytes;
the publisher must approve the actual placement and other applicable terms
under [Apache-2.0 §4](https://www.apache.org/licenses/LICENSE-2.0).

Do not copy [OWNER_CONFIRMATION.md](OWNER_CONFIRMATION.md) into the recipient
payload. An authorized signer must complete it privately, identify authority
for the exact derived weights and substantiate training-input permissions for
the intended distribution. Until then the EG79 component remains blocked.
