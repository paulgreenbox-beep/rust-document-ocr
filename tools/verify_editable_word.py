"""Independently inspect an exported, synthetic DOCX with XML and python-docx."""

import argparse
import io
import json
from pathlib import Path, PurePosixPath
import posixpath
import xml.etree.ElementTree as ET
import zipfile

from docx import Document
from PIL import Image

NS = {
    "w": "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
    "a": "http://schemas.openxmlformats.org/drawingml/2006/main",
    "r": "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
    "wp": "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing",
    "asvg": "http://schemas.microsoft.com/office/drawing/2016/SVG/main",
}


def q(prefix, local):
    return f"{{{NS[prefix]}}}{local}"


def inspect(path):
    native = Document(path)
    with zipfile.ZipFile(path) as archive:
        assert archive.testzip() is None, "CRC error"
        names = archive.namelist()
        assert len(names) == len(set(names)), "duplicate OPC member"
        assert all(not n.startswith("/") and ".." not in PurePosixPath(n).parts for n in names)
        root = ET.fromstring(archive.read("word/document.xml"))
        ct = ET.fromstring(archive.read("[Content_Types].xml"))
        mime = {child.attrib["Extension"]: child.attrib["ContentType"] for child in ct.findall("{*}Default")}
        rel = ET.fromstring(archive.read("word/_rels/document.xml.rels"))
        targets = {}
        for item in rel:
            rid = item.attrib["Id"]
            assert rid not in targets and item.get("TargetMode") != "External"
            target = posixpath.normpath(posixpath.join("word", item.attrib["Target"]))
            assert target in names, f"missing OPC target {target}"
            targets[rid] = target
        anchors = []
        media = {}
        used = set()
        for tag in ("anchor", "inline"):
            for drawing in root.findall(f".//wp:{tag}", NS):
                extent = drawing.find("wp:extent", NS)
                assert extent is not None and int(extent.get("cx")) > 0 and int(extent.get("cy")) > 0
                for image_tag in ("a:blip", "asvg:svgBlip"):
                    for blip in drawing.findall(f".//{image_tag}", NS):
                        target = targets[blip.attrib[q("r", "embed")]]
                        used.add(target)
                        if target in media:
                            continue
                        blob = archive.read(target)
                        ext = PurePosixPath(target).suffix.removeprefix(".")
                        if ext == "svg":
                            assert mime[ext] == "image/svg+xml"
                            svg = ET.fromstring(blob)
                            assert svg.tag == "{http://www.w3.org/2000/svg}svg"
                            assert len(svg.findall("{*}path")) > 0
                            media[target] = {"format": "SVG", "paths": len(svg.findall("{*}path"))}
                        else:
                            assert mime[ext] in {"image/png", "image/jpeg"}
                            with Image.open(io.BytesIO(blob)) as decoded:
                                assert decoded.format == {"image/png": "PNG", "image/jpeg": "JPEG"}[mime[ext]]
                                decoded.load()
                                media[target] = {"format": decoded.format, "size": decoded.size, "mode": decoded.mode}
                if drawing.find(".//asvg:svgBlip", NS) is not None:
                    fallback = drawing.find(".//a:blip", NS)
                    assert fallback is not None and targets[fallback.attrib[q("r", "embed")]].endswith(".png")
                anchors.append({"kind": tag, "extent_emu": [int(extent.get(k)) for k in ("cx", "cy")],
                                "position_emu": [drawing.findtext(f"wp:position{k}/wp:posOffset", namespaces=NS) for k in ("H", "V")]})
        assert used == {name for name in names if name.startswith("word/media/")}, "orphan media part"
        tables = root.findall("w:body/w:tbl", NS)
        assert len(tables) == len(native.tables), "independent Word reader saw a different table count"
        return {"paragraphs": [p.text for p in native.paragraphs if p.text],
                "table_rows": [[[c.text for c in row.cells] for row in table.rows] for table in native.tables],
                "horizontal_merges": len(root.findall(".//w:gridSpan", NS)),
                "vertical_merges": len(root.findall(".//w:vMerge", NS)),
                "drawings": anchors, "media": media,
                "section_sizes_twips": [{"width": size.get(q("w", "w")), "height": size.get(q("w", "h"))}
                                        for size in root.findall(".//w:sectPr/w:pgSz", NS)]}


if __name__ == "__main__":
    args = argparse.ArgumentParser(description=__doc__)
    args.add_argument("docx", type=Path)
    args.add_argument("--require-mixed", action="store_true")
    options = args.parse_args()
    result = inspect(options.docx)
    if options.require_mixed:
        assert result["paragraphs"] and len(result["table_rows"]) >= 2
        assert {"PNG", "SVG"}.issubset({entry["format"] for entry in result["media"].values()})
    print(json.dumps(result, ensure_ascii=True, indent=2))
