use std::fmt::Write as _;
use std::io::{Seek, Write};

use zip::result::ZipResult;
use zip::write::FileOptions;

use super::{
    PageGeometry, WordExportReport, WordPageGraphics, WordRasterFormat, WordRect, OFFICE_REL,
    PACKAGE_REL, XML_DECL,
};

const MAX_SVG_BYTES: usize = 256 * 1024;
const MAX_RASTER_BYTES: usize = 64 * 1024 * 1024;
const MAX_MEDIA_BYTES: usize = 96 * 1024 * 1024;
const MAX_MEDIA_PARTS: usize = 5000;
const EMU_PER_POINT: f64 = 12_700.0;
const EMU_PER_TWIP: f64 = 635.0;

pub(super) struct Package<'a> {
    media: Vec<Media<'a>>,
    media_bytes: usize,
    drawing_count: usize,
}

struct Media<'a> {
    extension: &'static str,
    bytes: &'a [u8],
}

impl<'a> Package<'a> {
    pub(super) fn new() -> Self {
        Self {
            media: Vec::new(),
            media_bytes: 0,
            drawing_count: 0,
        }
    }

    pub(super) fn add_graphics(
        &mut self,
        xml: &mut String,
        page: &'a WordPageGraphics,
        geometry: PageGeometry,
        scan_reference: bool,
        report: &mut WordExportReport,
    ) {
        if !valid_page_size(page) {
            return;
        }
        let mut paragraph_started = false;
        for (index, graphic) in page.images.iter().enumerate() {
            if graphic.scan_reference != scan_reference {
                continue;
            }
            if self.media.len() >= MAX_MEDIA_PARTS {
                report.warnings.push(format!(
                    "Word page {}: media part limit reached",
                    page.page_index + 1
                ));
                break;
            }
            let warn = |report: &mut WordExportReport, reason: &str| {
                report.warnings.push(format!(
                    "Word page {}, graphic {}: {reason}",
                    page.page_index + 1,
                    index + 1
                ));
            };
            if !valid_rect(graphic.rect, page) {
                warn(report, "invalid or out-of-page rectangle; omitted");
                continue;
            }
            let extension = match raster_extension(graphic.raster_format, &graphic.raster) {
                Some(extension) => extension,
                None => {
                    warn(report, "invalid or oversized raster; omitted");
                    continue;
                }
            };
            let Some(raster_id) = self.add_media(extension, &graphic.raster) else {
                warn(report, "document media budget reached; omitted");
                continue;
            };
            let svg_id = graphic.svg.as_deref().and_then(|svg| {
                if graphic.raster_format == WordRasterFormat::Png && bounded_svg(svg) {
                    let svg_id = self.add_media("svg", svg);
                    if svg_id.is_none() {
                        warn(report, "document media budget reached; used raster");
                    }
                    svg_id
                } else {
                    warn(
                        report,
                        "unsupported vector content or no PNG fallback; used raster",
                    );
                    None
                }
            });
            if !paragraph_started {
                if scan_reference {
                    super::paragraph(xml, "Scan reference", false);
                    xml.push_str(r#"<w:p><w:pPr><w:spacing w:before="0" w:after="0"/></w:pPr>"#);
                } else {
                    // A small, real Word paragraph owns the drawings. Source
                    // coordinates are attached to the page, not the paragraph.
                    xml.push_str(concat!(
                        r#"<w:p><w:pPr><w:spacing w:before="0" w:after="0" w:line="20" w:lineRule="exact"/>"#,
                        r#"<w:rPr><w:sz w:val="2"/><w:szCs w:val="2"/></w:rPr></w:pPr>"#,
                    ));
                }
                paragraph_started = true;
            }
            self.drawing_count += 1;
            let drawing = Drawing::new(graphic.rect, geometry, self.drawing_count, scan_reference);
            drawing.append(xml, raster_id, svg_id);
        }
        if paragraph_started {
            xml.push_str("</w:p>");
        }
    }

    fn add_media(&mut self, extension: &'static str, bytes: &'a [u8]) -> Option<usize> {
        if self.media.len() >= MAX_MEDIA_PARTS || bytes.len() > MAX_MEDIA_BYTES - self.media_bytes {
            return None;
        }
        self.media_bytes += bytes.len();
        self.media.push(Media { extension, bytes });
        Some(self.media.len())
    }

    pub(super) fn write_to_zip<W: Write + Seek>(
        self,
        zip: &mut zip::ZipWriter<W>,
    ) -> ZipResult<()> {
        let compressed =
            FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("word/_rels/document.xml.rels", compressed)?;
        write!(zip, r#"{XML_DECL}<Relationships xmlns="{PACKAGE_REL}">"#)?;
        for (offset, part) in self.media.iter().enumerate() {
            let id = offset + 1;
            write!(
                zip,
                r#"<Relationship Id="rId{id}" Type="{OFFICE_REL}/image" Target="media/image{id}.{}"/>"#,
                part.extension
            )?;
        }
        zip.write_all(b"</Relationships>")?;
        for (offset, part) in self.media.into_iter().enumerate() {
            let options = if part.extension == "svg" {
                compressed
            } else {
                FileOptions::default()
            };
            zip.start_file(
                format!("word/media/image{}.{}", offset + 1, part.extension),
                options,
            )?;
            zip.write_all(part.bytes)?;
        }
        Ok(())
    }
}

fn valid_page_size(page: &WordPageGraphics) -> bool {
    page.width_pt.is_finite()
        && page.height_pt.is_finite()
        && page.width_pt >= 1.0
        && page.height_pt >= 1.0
}

fn valid_rect(rect: WordRect, page: &WordPageGraphics) -> bool {
    [rect.x, rect.y, rect.width, rect.height]
        .into_iter()
        .all(f32::is_finite)
        && rect.x >= 0.0
        && rect.y >= 0.0
        && rect.width > 0.0
        && rect.height > 0.0
        && rect.x + rect.width <= page.width_pt + 0.05
        && rect.y + rect.height <= page.height_pt + 0.05
}

fn raster_extension(format: WordRasterFormat, bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() > MAX_RASTER_BYTES {
        return None;
    }
    match format {
        WordRasterFormat::Png
            if bytes.starts_with(b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR")
                && bytes.ends_with(b"\0\0\0\0IEND\xaeB`\x82") =>
        {
            Some("png")
        }
        WordRasterFormat::Jpeg
            if bytes.starts_with(b"\xff\xd8\xff") && bytes.ends_with(b"\xff\xd9") =>
        {
            Some("jpg")
        }
        _ => None,
    }
}

struct Drawing {
    id: usize,
    x: i64,
    y: i64,
    cx: i64,
    cy: i64,
    inline: bool,
}

impl Drawing {
    fn new(rect: WordRect, geometry: PageGeometry, id: usize, inline: bool) -> Self {
        let width = f64::from(rect.width);
        let height = f64::from(rect.height);
        let scale = if inline {
            // Leave room for a caption and line/section markers even when
            // the optional reference must flow onto the next Word page.
            let available_height = (geometry.height - 2 * geometry.margin_y)
                .saturating_sub(480)
                .max(20);
            geometry
                .scale
                .min(f64::from(geometry.content_width()) / (width * 20.0))
                .min(f64::from(available_height) / (height * 20.0))
        } else {
            geometry.scale
        };
        let emu = |points: f32| (f64::from(points) * scale * EMU_PER_POINT).round() as i64;
        let page_width = (f64::from(geometry.width) * EMU_PER_TWIP) as i64;
        let page_height = (f64::from(geometry.height) * EMU_PER_TWIP) as i64;
        let x = if inline {
            0
        } else {
            emu(rect.x).clamp(0, page_width - 1)
        };
        let y = if inline {
            0
        } else {
            emu(rect.y).clamp(0, page_height - 1)
        };
        Self {
            id,
            x,
            y,
            cx: emu(rect.width).clamp(1, page_width - x),
            cy: emu(rect.height).clamp(1, page_height - y),
            inline,
        }
    }

    fn append(&self, xml: &mut String, raster_id: usize, svg_id: Option<usize>) {
        let Self {
            id,
            x,
            y,
            cx,
            cy,
            inline,
        } = *self;
        xml.push_str("<w:r><w:drawing>");
        if inline {
            xml.push_str(r#"<wp:inline distT="0" distB="0" distL="0" distR="0">"#);
        } else {
            write!(xml, concat!(
                r#"<wp:anchor distT="0" distB="0" distL="0" distR="0" simplePos="0" relativeHeight="{}""#,
                r#" behindDoc="1" locked="0" layoutInCell="1" allowOverlap="1">"#,
                r#"<wp:simplePos x="0" y="0"/>"#,
                r#"<wp:positionH relativeFrom="page"><wp:posOffset>{}</wp:posOffset></wp:positionH>"#,
                r#"<wp:positionV relativeFrom="page"><wp:posOffset>{}</wp:posOffset></wp:positionV>"#,
            ), id, x, y).unwrap();
        }
        write!(
            xml,
            r#"<wp:extent cx="{cx}" cy="{cy}"/><wp:effectExtent l="0" t="0" r="0" b="0"/>"#
        )
        .unwrap();
        if !inline {
            xml.push_str("<wp:wrapNone/>");
        }
        write!(xml, concat!(
            r#"<wp:docPr id="{}" name="Picture {}"/>"#,
            r#"<wp:cNvGraphicFramePr><a:graphicFrameLocks noChangeAspect="1"/></wp:cNvGraphicFramePr>"#,
            r#"<a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:pic>"#,
            r#"<pic:nvPicPr><pic:cNvPr id="{}" name="Picture {}"/><pic:cNvPicPr/></pic:nvPicPr>"#,
            r#"<pic:blipFill><a:blip r:embed="rId{}">"#,
        ), id, id, id, id, raster_id).unwrap();
        if let Some(svg_id) = svg_id {
            write!(xml, r#"<a:extLst><a:ext uri="{{96DAC541-7B7A-43D3-8B79-37D633B846F1}}"><asvg:svgBlip r:embed="rId{svg_id}"/></a:ext></a:extLst>"#).unwrap();
        }
        write!(
            xml,
            concat!(
                r#"</a:blip><a:stretch><a:fillRect/></a:stretch></pic:blipFill>"#,
                r#"<pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{}" cy="{}"/></a:xfrm>"#,
                r#"<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr>"#,
                r#"</pic:pic></a:graphicData></a:graphic></wp:{}></w:drawing></w:r>"#,
            ),
            cx,
            cy,
            if inline { "inline" } else { "anchor" }
        )
        .unwrap();
    }
}

// Accept only the program's path-only SVG subset. No external references,
// arbitrary XML, style, scripts, entities, or user-controlled tag names.
fn bounded_svg(bytes: &[u8]) -> bool {
    if bytes.len() > MAX_SVG_BYTES {
        return false;
    }
    let Ok(svg) = std::str::from_utf8(bytes) else {
        return false;
    };
    let Some(svg) = svg
        .trim()
        .strip_prefix("<svg ")
        .and_then(|s| s.strip_suffix("</svg>"))
    else {
        return false;
    };
    let Some((root, mut children)) = svg.split_once('>') else {
        return false;
    };
    let mut namespace = false;
    let mut view_box = false;
    if !attributes(root, |name, value| match name {
        "xmlns" if !namespace && value == "http://www.w3.org/2000/svg" => {
            namespace = true;
            true
        }
        "viewBox" if !view_box && valid_view_box(value) => {
            view_box = true;
            true
        }
        _ => false,
    }) || !namespace
        || !view_box
    {
        return false;
    }
    let mut paths = 0;
    while !children.trim().is_empty() {
        let Some((path, rest)) = children
            .trim_start()
            .strip_prefix("<path ")
            .and_then(|s| s.split_once("/>"))
        else {
            return false;
        };
        let mut seen = 0u16;
        if !attributes(path, |name, value| {
            let (bit, valid) = match name {
                "d" => (0, bounded_path(value)),
                "fill" => (1, color(value)),
                "stroke" => (2, color(value)),
                "fill-rule" => (3, matches!(value, "nonzero" | "evenodd")),
                "fill-opacity" => (4, bounded_number(value, 0.0, 1.0)),
                "stroke-opacity" => (5, bounded_number(value, 0.0, 1.0)),
                "stroke-width" => (6, bounded_number(value, 0.0, 100_000.0)),
                "stroke-linecap" => (7, matches!(value, "butt" | "round" | "square")),
                "stroke-linejoin" => (8, matches!(value, "miter" | "round" | "bevel")),
                "stroke-miterlimit" => (9, bounded_number(value, 0.0, 100_000.0)),
                _ => return false,
            };
            let mask = 1 << bit;
            if !valid || seen & mask != 0 {
                return false;
            }
            seen |= mask;
            true
        }) || seen & 1 == 0
        {
            return false;
        }
        paths += 1;
        if paths > 256 {
            return false;
        }
        children = rest;
    }
    paths > 0
}

fn attributes(mut input: &str, mut accept: impl FnMut(&str, &str) -> bool) -> bool {
    while !input.trim().is_empty() {
        let Some((name, after_equals)) = input.trim_start().split_once("=\"") else {
            return false;
        };
        let Some((value, rest)) = after_equals.split_once('"') else {
            return false;
        };
        if !accept(name, value) || (!rest.is_empty() && !rest.starts_with(char::is_whitespace)) {
            return false;
        }
        input = rest;
    }
    true
}

fn color(value: &str) -> bool {
    value == "none"
        || value
            .strip_prefix('#')
            .is_some_and(|hex| hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
}

fn bounded_number(value: &str, min: f64, max: f64) -> bool {
    value.parse::<f64>().is_ok_and(|n| (min..=max).contains(&n))
}

fn bounded_path(value: &str) -> bool {
    let mut tokens = value.split_ascii_whitespace();
    let mut has_move = false;
    let mut has_geometry = false;
    let mut command_count = 0;
    while let Some(command) = tokens.next() {
        command_count += 1;
        if command_count > 1024 {
            return false;
        }
        let arguments = match command {
            "M" => {
                has_move = true;
                2
            }
            "L" if has_move => {
                has_geometry = true;
                2
            }
            "C" if has_move => {
                has_geometry = true;
                6
            }
            "Z" if has_move => 0,
            _ => return false,
        };
        if (0..arguments).any(|_| {
            !tokens
                .next()
                .is_some_and(|n| bounded_number(n, -100_000.0, 100_000.0))
        }) {
            return false;
        }
    }
    has_geometry
}

fn valid_view_box(value: &str) -> bool {
    let mut pieces = value.split_ascii_whitespace();
    pieces.next() == Some("0")
        && pieces.next() == Some("0")
        && pieces
            .next()
            .is_some_and(|p| bounded_number(p, 0.00001, 100_000.0))
        && pieces
            .next()
            .is_some_and(|p| bounded_number(p, 0.00001, 100_000.0))
        && pieces.next().is_none()
}

#[cfg(test)]
mod tests {
    use super::bounded_svg;

    #[test]
    fn only_generated_path_svg_is_embedded() {
        let root = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 20">"#;
        let path = r##"<path d="M 0 0 L 10 20 Z" fill="#abcdef" stroke="none"/>"##;
        let svg = format!("{root}{path}</svg>");
        assert!(bounded_svg(svg.as_bytes()));
        for unsafe_xml in [
            "<!DOCTYPE svg>",
            "<script/>",
            "<image href=\"x\"/>",
            r#"<path d="M 0 0" fill="url(foo)"/>"#,
        ] {
            let svg = format!("{root}{path}{unsafe_xml}</svg>");
            assert!(!bounded_svg(svg.as_bytes()));
        }
    }
}
