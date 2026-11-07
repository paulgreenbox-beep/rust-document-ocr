//! Small synthetic HEIF writer; no image encoder, unsafe code or private data.

use heic_rs::context::Context;

pub fn bx(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = u32::try_from(body.len() + 8)
        .unwrap()
        .to_be_bytes()
        .to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    out
}

fn full(kind: &[u8; 4], version: u8, flags: u32, body: &[u8]) -> Vec<u8> {
    let mut inner = vec![version];
    inner.extend_from_slice(&flags.to_be_bytes()[1..]);
    inner.extend_from_slice(body);
    bx(kind, &inner)
}

#[derive(Clone)]
pub struct Item {
    pub width: u32,
    pub height: u32,
    pub depth: u8,
    pub chroma: u8,
    pub hvcc: Vec<u8>,
    pub data: Vec<u8>,
    pub extra: Vec<Vec<u8>>,
    pub alpha: bool,
}

impl Item {
    pub fn from_file(bytes: &[u8]) -> Self {
        let context = Context::open(bytes).unwrap();
        let id = context.meta.primary;
        let props = context.props(id).unwrap();
        let config = props.hvcc.as_ref().expect("one coded HEVC image");
        let size = props.ispe.unwrap();
        let hvcc = context.meta.props.find(id, b"hvcC").unwrap();
        let extra = context
            .meta
            .props
            .find_all(id, b"colr")
            .map(|property| bx(b"colr", property.payload))
            .collect();
        Self {
            width: size.width,
            height: size.height,
            depth: config.bit_depth_luma,
            chroma: config.chroma_format,
            hvcc: bx(b"hvcC", hvcc.payload),
            data: context.item_data(id).unwrap().into_owned(),
            extra,
            alpha: false,
        }
    }
}

/// No inherited irot=0: transformative properties are added explicitly.
pub fn transformed(bytes: &[u8], kind: &[u8; 4], code: u8) -> Vec<u8> {
    let mut item = Item::from_file(bytes);
    item.extra.push(bx(kind, &[code]));
    file(&[item])
}

pub fn hvcc(sets: &[Vec<u8>], profile: u8, depth: u8, chroma: u8) -> Vec<u8> {
    let mut body = vec![1, profile];
    body.extend_from_slice(&(1u32 << (31 - profile)).to_be_bytes());
    body.extend_from_slice(&[0xd0, 0, 0, 0, 0, 0]);
    body.push(30);
    body.extend_from_slice(&0xf000u16.to_be_bytes());
    body.extend_from_slice(&[0xfc, 0xfc | chroma, 0xf8 | (depth - 8), 0xf8 | (depth - 8)]);
    body.extend_from_slice(&0u16.to_be_bytes());
    body.push(0x0f); // One nested temporal layer; four-byte NAL lengths.
    body.push(sets.len() as u8);
    for nal in sets {
        let kind = (nal[0] >> 1) & 0x3f;
        body.push(0x80 | kind);
        body.extend_from_slice(&1u16.to_be_bytes());
        body.extend_from_slice(&(nal.len() as u16).to_be_bytes());
        body.extend_from_slice(nal);
    }
    bx(b"hvcC", &body)
}

fn metadata(items: &[Item], first_payload: u32) -> Vec<u8> {
    let mut handler = vec![0; 4];
    handler.extend_from_slice(b"pict");
    handler.extend_from_slice(&[0; 13]);
    let mut children = full(b"hdlr", 0, 0, &handler);
    children.extend(full(b"pitm", 0, 0, &1u16.to_be_bytes()));
    let mut catalogue = (items.len() as u16).to_be_bytes().to_vec();
    let mut locations = vec![0x44, 0x00]; // Four-byte extent offset/length, no base.
    locations.extend_from_slice(&(items.len() as u16).to_be_bytes());
    let mut properties = Vec::new();
    let mut associations = (items.len() as u32).to_be_bytes().to_vec();
    let mut next_property = 1u8;
    let mut offset = first_payload;
    let mut references = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let id = (index + 1) as u16;
        let mut info = id.to_be_bytes().to_vec();
        info.extend_from_slice(&0u16.to_be_bytes());
        info.extend_from_slice(b"hvc1\0");
        catalogue.extend(full(b"infe", 2, u32::from(item.alpha), &info));
        locations.extend_from_slice(&id.to_be_bytes());
        locations.extend_from_slice(&0u16.to_be_bytes()); // data_reference_index
        locations.extend_from_slice(&1u16.to_be_bytes()); // extent_count
        locations.extend_from_slice(&offset.to_be_bytes());
        locations.extend_from_slice(&(item.data.len() as u32).to_be_bytes());
        offset += item.data.len() as u32;
        let mut size = item.width.to_be_bytes().to_vec();
        size.extend_from_slice(&item.height.to_be_bytes());
        let channels = if item.chroma == 0 { 1 } else { 3 };
        let mut pixi = vec![channels];
        pixi.extend(std::iter::repeat_n(item.depth, channels as usize));
        let mut item_properties = vec![
            full(b"ispe", 0, 0, &size),
            item.hvcc.clone(),
            full(b"pixi", 0, 0, &pixi),
        ];
        item_properties.extend(item.extra.clone());
        if item.alpha {
            item_properties.push(full(b"auxC", 0, 0, b"urn:mpeg:hevc:2015:auxid:1\0"));
            let mut link = id.to_be_bytes().to_vec();
            link.extend_from_slice(&1u16.to_be_bytes());
            link.extend_from_slice(&1u16.to_be_bytes());
            references.extend(bx(b"auxl", &link));
        }
        associations.extend_from_slice(&id.to_be_bytes());
        associations.push(item_properties.len() as u8);
        for property in item_properties {
            assert!(next_property < 128);
            associations.push(0x80 | next_property);
            next_property += 1;
            properties.extend(property);
        }
    }
    children.extend(full(b"iinf", 0, 0, &catalogue));
    children.extend(full(b"iloc", 0, 0, &locations));
    let mut iprp = bx(b"ipco", &properties);
    iprp.extend(full(b"ipma", 0, 0, &associations));
    children.extend(bx(b"iprp", &iprp));
    if !references.is_empty() {
        children.extend(full(b"iref", 0, 0, &references));
    }
    full(b"meta", 0, 0, &children)
}

pub fn file(items: &[Item]) -> Vec<u8> {
    assert!(!items.is_empty() && items.len() <= 2);
    let brand = if items.iter().any(|item| item.depth > 8 || item.chroma != 1) {
        b"heix"
    } else {
        b"heic"
    };
    let mut file_type = brand.to_vec();
    file_type.extend_from_slice(&0u32.to_be_bytes());
    file_type.extend_from_slice(b"mif1");
    file_type.extend_from_slice(brand);
    let mut out = bx(b"ftyp", &file_type);
    let empty_meta = metadata(items, 0);
    let offset = (out.len() + empty_meta.len() + 8) as u32;
    let meta = metadata(items, offset);
    assert_eq!(meta.len(), empty_meta.len());
    out.extend(meta);
    let pixels = items
        .iter()
        .flat_map(|item| item.data.iter().copied())
        .collect::<Vec<_>>();
    out.extend(bx(b"mdat", &pixels));
    out
}
