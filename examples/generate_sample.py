"""Generate original synthetic documents; no customer data or bundled fonts.

Development-only requirements: reportlab, Pillow and pypdfium2.
The first PDF uses the standard, unembedded Helvetica PDF font. PDFium renders
that original page to create the image-only second PDF. No font files are copied.
"""
from pathlib import Path
from reportlab.pdfgen import canvas
from reportlab.lib.utils import ImageReader
import pypdfium2 as pdfium

ROOT = Path(__file__).resolve().parent / "documents"
ROOT.mkdir(parents=True, exist_ok=True)
native_path = ROOT / "synthetic-text-table.pdf"
raster_path = ROOT / "synthetic-raster.pdf"

doc = canvas.Canvas(str(native_path), pagesize=(612, 792), invariant=1)
doc.setTitle("Synthetic OCR text and table fixture")
doc.setAuthor("Synthetic fixture")
doc.setCreator("Rust Document OCR synthetic sample generator")
doc.setFont("Helvetica-Bold", 18)
doc.drawString(54, 738, "RUST DOCUMENT OCR - SYNTHETIC SAMPLE")
doc.setFont("Helvetica", 12)
doc.drawString(54, 706, "This document contains original test text and a small ruled table.")
doc.drawString(54, 686, "It contains no customer information, personal data, or real invoice.")
xs = [54, 234, 414, 558]
ys = [620, 572, 524, 476]
doc.setLineWidth(1.2)
for x in xs:
    doc.line(x, ys[-1], x, ys[0])
for y in ys:
    doc.line(xs[0], y, xs[-1], y)
rows = [("Item", "Quantity", "Unit"), ("Alpha", "12", "pieces"), ("Beta", "7", "metres")]
for row, values in enumerate(rows):
    doc.setFont("Helvetica-Bold" if row == 0 else "Helvetica", 13)
    for col, value in enumerate(values):
        doc.drawString(xs[col] + 12, ys[row] - 29, value)
doc.setFont("Helvetica", 11)
doc.drawString(54, 438, "Geometry and recognized text are separate outputs.")
doc.drawString(54, 418, "Synthetic examples demonstrate operation, not real-world OCR accuracy.")
doc.save()

pdf = pdfium.PdfDocument(str(native_path))
page = pdf[0]
bitmap = page.render(scale=300 / 72)
image = bitmap.to_pil().convert("RGB")
raster = canvas.Canvas(str(raster_path), pagesize=(612, 792), invariant=1)
raster.setTitle("Synthetic image-only OCR fixture")
raster.setAuthor("Synthetic fixture")
raster.setCreator("Rust Document OCR synthetic sample generator")
raster.drawImage(ImageReader(image), 0, 0, width=612, height=792)
raster.save()
image.close()
bitmap.close()
page.close()
pdf.close()
print(native_path.name)
print(raster_path.name)
