# EG79 release confirmation — unsigned template

**Status: training-data origin reported by the project owner; redistribution
rights PENDING.** Complete and retain the signed record privately. Do not
commit the signed record, evidence, model weights or training data. This is
not an approval.

Scope: inclusion of `tessdata/eng_technical_eg79_v1.traineddata`,
SHA-256 `7ceee3f56f290c3b90c1670d8e0c7dd86e344759ff03915407c53369f1c8437c`,
in the named Windows x64 package only. This confirmation does not authorize
publishing a training corpus or a standalone weight download.

## Training and test data origin reported by the project owner

- **Fine-tuning:** EG79 used the owner's own scanned documents and
  independently prepared materials. The owner identifies real raster text
  from the scans as the most useful material, including words, text lines,
  technical designations, and table cells.
- **Letterforms in source documents:** The owner describes letterforms
  consistent with Times New Roman, Calibri, and ISOCPEUR (AutoCAD).
  Fine-tuning used raster images of documents. No font files are intended
  for inclusion in the distributed Windows OCR package.
- **Separate test material:** Arial was used early to create a synthetic
  raster PDF for OCR testing. Its use for fine-tuning EG79 is not confirmed;
  it is recorded as test material, not as an established training input.

This account is based on the owner's statement, not an independent
verification of the datasets, font identities, or rights. Permission to
use underlying document content and distribute the resulting weights
requires separate confirmation.

The base for EG79 is Tesseract `tessdata_best/eng.traineddata` under
Apache-2.0; see the [pinned base and license evidence](README.md#evidence-and-correction).
The base model and third-party software retain their original licenses
and copyrights.

## Confirmations still required

- Publisher's legal name and intended release/channel/territories: __________
- Rightsholder(s) of the owner's training data and of the local model changes: __________
- Authority of the signer/publisher: __________
- Exact proposed release manifest SHA-256 (after notice assembly): __________
- Private evidence reference(s) (no data, crops, labels or credentials here): __________

For each item, record **CONFIRMED**, **NOT CONFIRMED** or **NOT APPLICABLE**
with the relevant basis and any restrictions. Explain every NOT APPLICABLE.

| Confirmation required | Decision and evidence reference |
| --- | --- |
| Publisher has documented authority to redistribute these exact fine-tuned weights in this package and scope. Identify the applicable terms for local modifications; the base license alone is not the evidence for those rights. | __________ |
| The project had the necessary rights to use the underlying content of the scanned documents and any independently prepared inputs actually used for fine-tuning. Identify applicable permissions and restrictions for raster crops, labels, fonts and images where used. Assess separate test materials independently if they are proposed for distribution. | __________ |
| No applicable contract, license, confidentiality term or other restriction prevents distribution of the resulting weights in the intended scope. Resolve any exceptions before signing. | __________ |
| The final package identifies the modified model conspicuously and includes the pinned `tessdata_best` license and applicable attributions/notices. Confirm the actual recipient placement and any additional terms or credits. | __________ |

Decision: **PENDING / APPROVE EXACT SCOPE / BLOCK** (select one): __________

Authorized name, capacity, signature and date: __________

Any unresolved item blocks approval of EG79. Re-review if the model, applicable
rights, notice placement, intended scope or final package identity changes.
This record concerns EG79 only; other release gates remain independent.
