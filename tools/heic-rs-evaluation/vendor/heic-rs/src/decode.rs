// Modified 2026-11-07 for the OCR research derivative: aggregate raw-picture preflight.
//! [`decode`]: container to pixels.
//!
//! Resolving the primary item, splitting NAL units, decoding each coded
//! picture through [`crate::hevc::decode_still`], composing a grid, converting
//! colour, applying transforms — in that order, and with the declared size
//! checked against the caller's ceiling before HEVC image buffers are allocated.
//! Encoded-input and metadata parsing allocations are a separate concern.

use alloc::vec::Vec;

use crate::color::{self, Source};
use crate::context::Context;
use crate::error::{Error, ResourceStage, Result};
use crate::grid::{self, Grid, Mosaic};
use crate::hevc::{self, Frame};
use crate::image::{DecodeOptions, Image, check_pixels};
use crate::parallel;
use crate::props::ItemProps;
use crate::props::colr::Nclx;
use crate::transform;

/// Decode the primary image of a HEIF file.
///
/// # Errors
///
/// Container-level failures are reported before HEVC image decoding; metadata
/// parsing still has an input-dependent cost. A bitstream using a coding tool
/// this decoder does not implement comes back as [`Error::Unsupported`] naming
/// that tool.
pub fn decode(bytes: &[u8], options: &DecodeOptions) -> Result<Image> {
    let ctx = Context::open(bytes)?;
    let id = ctx.meta.primary;
    let p = ctx.props(id)?;
    if options.strict && ctx.meta.props.unknown_essential(id).is_some() {
        return Err(Error::Unsupported(
            "the primary item carries an essential property this build does not implement",
        ));
    }
    let limit = options.pixel_limit();
    let (cw, ch) = ctx.coded_size(id, &p)?;
    // Refuse the declared size before decoding anything, so that a small file
    // claiming an enormous image costs nothing.
    check_pixels(cw, ch, limit)?;
    let (tw, th) = transform::transformed_size(cw, ch, &p.transforms)?;
    check_pixels(tw, th, limit)?;

    // Immutable, serial preflight of every picture, including auxiliary alpha
    // and each tile occurrence. Reject the complete request before any HEVC
    // item payload copies, image buffers, parallel dispatch or private pool.
    // Metadata parsing/encoded byte bounds remain a separate input concern.
    let mut budget = CodedBudget { used: 0, limit };
    preflight_item(&ctx, id, &p, &mut budget)?;
    if options.decode_alpha && options.layout.has_alpha() {
        if let Some(aux) = ctx.alpha_item(id)? {
            preflight_item(&ctx, aux, &ctx.props(aux)?, &mut budget)?;
        }
    }
    parallel::scope(options.threads, |threads| run(&ctx, &p, options, threads))
}

struct CodedBudget {
    used: u64,
    limit: u64,
}

impl CodedBudget {
    fn charge(&mut self, pixels: u64) -> Result<()> {
        let required = self.used.checked_add(pixels).ok_or(Error::ResourceLimit {
            stage: ResourceStage::AggregateCodedPictures,
            required: u64::MAX,
            limit: self.limit,
        })?;
        if required > self.limit {
            return Err(Error::ResourceLimit {
                stage: ResourceStage::AggregateCodedPictures,
                required,
                limit: self.limit,
            });
        }
        self.used = required;
        Ok(())
    }
}

fn preflight_item(
    ctx: &Context<'_>,
    id: u32,
    p: &ItemProps<'_>,
    budget: &mut CodedBudget,
) -> Result<()> {
    if let Some((grid, tiles)) = ctx.grid(id)? {
        // A forged primary ispe must not hide a larger grid/alpha canvas.
        check_pixels(grid.output_width, grid.output_height, budget.limit)?;
        for tile in tiles {
            preflight_coded(&ctx.props(tile)?, budget)?;
        }
    } else {
        preflight_coded(p, budget)?;
    }
    Ok(())
}

fn preflight_coded(p: &ItemProps<'_>, budget: &mut CodedBudget) -> Result<()> {
    let hvcc = p.hvcc.as_ref().ok_or(Error::MissingBox("hvcC"))?;
    // Charge the largest raw area across all supplied SPS configurations,
    // not the cropped probe geometry or the container's ispe metadata.
    let required = hevc::coded_pixel_requirement(&hvcc.parameter_sets(), budget.limit)?;
    budget.charge(required)
}

/// The image-buffer work, entered only after the whole immutable request fits.
fn run(
    ctx: &Context<'_>,
    p: &ItemProps<'_>,
    options: &DecodeOptions,
    threads: Option<usize>,
) -> Result<Image> {
    let id = ctx.meta.primary;
    let limit = options.pixel_limit();

    let decoded = decode_item(ctx, id, p, limit, threads)?;
    let alpha = alpha_frame(ctx, id, options, limit, threads)?;
    let nclx = p.nclx.unwrap_or_default();
    let (layout, alpha) = (options.layout, alpha.as_ref());
    let image = match &decoded {
        Decoded::Picture(frame) => {
            color::convert_source(&Source::Frame(frame), alpha, nclx, layout, limit, threads)?
        }
        // The tiles are read in place: a canvas would be allocated, zeroed
        // and filled only for the colour pass to read it straight back.
        Decoded::Tiles(g, tiles) => {
            let mosaic = Mosaic::new(g, tiles)?;
            color::convert_source(
                &Source::Mosaic(&mosaic),
                alpha,
                nclx,
                layout,
                limit,
                threads,
            )?
        }
    };
    if options.apply_transforms {
        transform::apply_all(image, &p.transforms)
    } else {
        Ok(image)
    }
}

/// What decoding an item produced.
enum Decoded {
    /// One coded picture, validated.
    Picture(Frame),
    /// A grid's tiles in `dimg` order, each validated, not yet composed.
    Tiles(Grid, Vec<Frame>),
}

impl Decoded {
    /// One frame, composing a grid's tiles if there are any.
    fn into_frame(self, limit: u64) -> Result<Frame> {
        match self {
            Decoded::Picture(frame) => Ok(frame),
            Decoded::Tiles(g, tiles) => grid::compose(&g, &tiles, limit),
        }
    }
}

/// Decode one item, following a `grid` derivation when there is one.
///
/// `p` is the item's own properties, gathered once by the caller; a single
/// coded picture is decoded straight from them rather than reading its
/// `iprp` entry a second time. A grid's tiles are independent coded pictures:
/// nothing in one tile's bitstream refers to another, so they are decoded
/// together when the caller allows it. The tiles come back in `dimg` order,
/// which is what [`Mosaic`] and [`grid::compose`] place them by, so the result
/// does not depend on the order they happened to finish in.
fn decode_item(
    ctx: &Context<'_>,
    id: u32,
    p: &ItemProps<'_>,
    limit: u64,
    threads: Option<usize>,
) -> Result<Decoded> {
    match ctx.grid(id)? {
        Some((g, tiles)) => {
            let frames = parallel::try_map(&tiles, threads, |t| {
                decode_coded(ctx, *t, &ctx.props(*t)?, limit)
            })?;
            Ok(Decoded::Tiles(g, frames))
        }
        None => decode_coded(ctx, id, p, limit).map(Decoded::Picture),
    }
}

/// Decode a single coded picture item through the codec seam.
fn decode_coded(ctx: &Context<'_>, id: u32, p: &ItemProps<'_>, limit: u64) -> Result<Frame> {
    let hvcc = p.hvcc.as_ref().ok_or(Error::MissingBox("hvcC"))?;
    let data = ctx.item_data(id)?;
    let nals = hvcc.split_nals(&data)?;
    let params = hvcc.parameter_sets();
    if params.is_empty() {
        return Err(Error::Malformed("hvcC carries no parameter sets"));
    }
    let frame = hevc::decode_still_limited(&params, &nals, limit)?;
    frame.validate()?;
    Ok(frame)
}

/// Decode the alpha auxiliary item, when the caller asked for alpha and the
/// requested layout has somewhere to put it.
fn alpha_frame(
    ctx: &Context<'_>,
    id: u32,
    options: &DecodeOptions,
    limit: u64,
    threads: Option<usize>,
) -> Result<Option<Frame>> {
    if !options.decode_alpha || !options.layout.has_alpha() {
        return Ok(None);
    }
    let Some(aux) = ctx.alpha_item(id)? else {
        return Ok(None);
    };
    let aux_props = ctx.props(aux)?;
    // The alpha kernel reads one plane, so a tiled alpha item is composed;
    // alpha grids are rare enough that the canvas is not worth avoiding.
    decode_item(ctx, aux, &aux_props, limit, threads)?
        .into_frame(limit)
        .map(Some)
}

/// The colour description that will be used for an item, including the
/// documented fallback for files that carry no `colr`.
pub fn effective_nclx(p: &ItemProps<'_>) -> Nclx {
    p.nclx.unwrap_or_default()
}

#[cfg(test)]
mod budget_tests {
    use super::*;

    #[test]
    fn resource_limit_aggregate_addition_does_not_wrap_or_mutate_on_failure() {
        let mut budget = CodedBudget {
            used: u64::MAX - 1,
            limit: u64::MAX,
        };
        assert_eq!(
            budget.charge(2),
            Err(Error::ResourceLimit {
                stage: ResourceStage::AggregateCodedPictures,
                required: u64::MAX,
                limit: u64::MAX,
            })
        );
        assert_eq!(budget.used, u64::MAX - 1);
    }
}
