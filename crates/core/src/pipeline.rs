//! The whole pipeline, in the exact SPEC 9.1 order. `sniff` runs first,
//! so a refused format never reaches the decoder.
//!
//! Fail closed (S-6): the output is written to the private folder, read
//! back and verified. If the check fails, the file is deleted and an error
//! is returned. The original is never used as a fallback.

use std::io::Read;
use std::path::{Path, PathBuf};

use image::{RgbaImage, imageops};

use crate::color::SourceColor;
use crate::decode::{Decoder, GlycinDecoder};
use crate::encode::{Encoder, JpegEncoder, OutputProfile, Pixels, PngEncoder};
use crate::output::{PrivateDir, file_name};
use crate::summary::{self, Summary};
use crate::{Error, Limits, Options, OutputFormat, color, edit, fit, orient, sniff, verify};

/// The profile embedded in every output: none. The pixels are sRGB, which
/// is what readers assume without a profile (SPEC 9.1 step 5).
const OUTPUT_PROFILE: Option<OutputProfile> = None;

/// Where one output goes: `position` (from 1) of `total` in a batch.
#[derive(Debug, Clone, Copy)]
pub struct Target<'a> {
    pub dir: &'a PrivateDir,
    pub position: usize,
    pub total: usize,
}

/// A verified output file in the private folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prepared {
    pub path: PathBuf,
    pub summary: Summary,
}

/// The encoder for each output format. Tests swap it.
pub type EncoderFor<'a> = &'a dyn Fn(OutputFormat) -> Result<Box<dyn Encoder + Sync>, Error>;

/// Prepares `input` with the glycin decoder and our encoders.
pub fn prepare(input: &Path, opts: &Options, target: Target<'_>) -> Result<Prepared, Error> {
    prepare_with(
        input,
        opts,
        target,
        &Limits::default(),
        &GlycinDecoder,
        &default_encoder,
    )
}

/// JPEG and PNG. WebP output is not in M1.
pub fn default_encoder(format: OutputFormat) -> Result<Box<dyn Encoder + Sync>, Error> {
    match format {
        OutputFormat::Jpeg => Ok(Box::new(JpegEncoder)),
        OutputFormat::Png => Ok(Box::new(PngEncoder)),
        OutputFormat::Webp => Err(Error::Encode(
            "WebP output is not supported in this version",
        )),
        OutputFormat::Auto => Err(Error::Encode("no output format was chosen")),
    }
}

/// [`prepare`] with a chosen decoder and encoders.
pub fn prepare_with(
    input: &Path,
    opts: &Options,
    target: Target<'_>,
    limits: &Limits,
    decoder: &dyn Decoder,
    encoder_for: EncoderFor<'_>,
) -> Result<Prepared, Error> {
    // 1. and 2.
    let upright = decode_upright(input, limits, decoder)?;
    let input_size = upright.stored_size;
    let (input_bytes, removed) = scan_input(input, limits)?;
    // 3. and 4. Crop, then burn in the redactions (FR-5).
    let image = edit::apply(upright.image, opts.crop, &opts.redactions)?;
    // 5. Convert to sRGB; the source profile is dropped here.
    let image = color::to_srgb(image, upright.color.as_ref())?;
    // 6. Fit-to-size encode.
    let px = Pixels::new(image, OUTPUT_PROFILE);
    let format = choose_format(opts.format, px.has_alpha);
    let encoder = encoder_for(format)?;
    let fitted = fit::fit(&px, opts.target_bytes, opts.max_long_edge, encoder.as_ref())?;
    // Leaky bytes are never written, not even for a moment.
    verify_output(&fitted.bytes)?;
    // 7. Write to the private folder with a neutral name.
    let name = output_name(input, opts, target, encoder.extension());
    let path = target.dir.write(&name, &fitted.bytes)?;
    // 8. Read back and verify. Fail closed (S-6): the verify error wins
    // even if the file cannot be removed.
    if let Err(e) = verify_file(&path) {
        let _ = std::fs::remove_file(&path);
        return Err(e);
    }
    Ok(Prepared {
        path,
        summary: Summary {
            input_bytes,
            output_bytes: fitted.bytes.len() as u64,
            input_size,
            output_size: (fitted.width, fitted.height),
            removed,
        },
    })
}

/// What the editor shows: the oriented sRGB pixels, downscaled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preview {
    /// Long edge at most the asked `max_edge`; never upscaled.
    pub pixels: RgbaImage,
    /// The oriented size at full resolution. Edit coordinates are in
    /// this size (SPEC 9.4).
    pub full_size: (u32, u32),
}

/// SPEC 9.1 steps 1, 2 and 5 with the glycin decoder: the same decode,
/// orientation and color conversion as [`prepare`], then a downscale. No
/// encode, and no file is written.
pub fn preview(input: &Path, limits: &Limits, max_edge: u32) -> Result<Preview, Error> {
    preview_with(input, limits, max_edge, &GlycinDecoder)
}

/// [`preview`] with a chosen decoder.
pub fn preview_with(
    input: &Path,
    limits: &Limits,
    max_edge: u32,
    decoder: &dyn Decoder,
) -> Result<Preview, Error> {
    let upright = decode_upright(input, limits, decoder)?;
    let image = color::to_srgb(upright.image, upright.color.as_ref())?;
    let full_size = image.dimensions();
    Ok(Preview {
        pixels: downscale(image, max_edge),
        full_size,
    })
}

/// Steps 1 and 2, shared by [`prepare_with`] and [`preview_with`] so the
/// preview and the output are always oriented the same way.
struct Upright {
    image: RgbaImage,
    stored_size: (u32, u32),
    color: Option<SourceColor>,
}

fn decode_upright(input: &Path, limits: &Limits, decoder: &dyn Decoder) -> Result<Upright, Error> {
    sniff::check_file(input)?;
    // 1. Decode in the sandbox.
    let decoded = decoder.decode(input, limits)?;
    let stored_size = decoded.pixels.dimensions();
    // 2. Orientation, before the metadata is gone (FR-2).
    let image = orient::apply(decoded.pixels, decoded.orientation);
    Ok(Upright {
        image,
        stored_size,
        color: decoded.color,
    })
}

fn downscale(image: RgbaImage, max_edge: u32) -> RgbaImage {
    let (w, h) = image.dimensions();
    let max_edge = max_edge.max(1);
    let long = w.max(h);
    if long <= max_edge {
        return image;
    }
    let scale = |n: u32| {
        ((u64::from(n) * u64::from(max_edge) + u64::from(long) / 2) / u64::from(long)).max(1) as u32
    };
    imageops::thumbnail(&image, scale(w), scale(h))
}

/// Auto keeps real transparency as PNG (SPEC 4, 9.3). Everything else
/// becomes JPEG.
fn choose_format(wanted: OutputFormat, has_alpha: bool) -> OutputFormat {
    match wanted {
        OutputFormat::Auto if has_alpha => OutputFormat::Png,
        OutputFormat::Auto => OutputFormat::Jpeg,
        f => f,
    }
}

fn output_name(input: &Path, opts: &Options, target: Target<'_>, ext: &str) -> String {
    file_name(input, opts.keep_name, target.position, target.total, ext)
}

/// The input size and the metadata kinds in it, for the summary (FR-8).
fn scan_input(
    input: &Path,
    limits: &Limits,
) -> Result<(u64, Option<Vec<summary::Removed>>), Error> {
    let mut bytes = Vec::new();
    std::fs::File::open(input)?
        .take(limits.max_file_bytes.saturating_add(1))
        .read_to_end(&mut bytes)?;
    limits.check_file_size(bytes.len() as u64)?;
    Ok((bytes.len() as u64, summary::scan(&bytes)))
}

fn verify_file(path: &Path) -> Result<(), Error> {
    verify_output(&std::fs::read(path)?)
}

/// Checks output bytes against what the pipeline writes: image data and
/// only the output profile (S-6). Callers that hold output bytes run this
/// again before they hand them on.
pub fn verify_output(bytes: &[u8]) -> Result<(), Error> {
    verify::verify(bytes, OUTPUT_PROFILE.map(OutputProfile::bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downscale_keeps_the_aspect_and_never_upscales() {
        let size = |w, h, edge| downscale(RgbaImage::new(w, h), edge).dimensions();
        assert_eq!(size(4000, 3000, 2048), (2048, 1536));
        assert_eq!(size(3000, 4000, 2048), (1536, 2048));
        assert_eq!(size(100, 50, 2048), (100, 50));
        assert_eq!(size(5000, 1, 100), (100, 1));
        assert_eq!(size(10, 10, 0), (1, 1));
    }

    #[test]
    fn auto_keeps_alpha_as_png() {
        assert_eq!(choose_format(OutputFormat::Auto, true), OutputFormat::Png);
        assert_eq!(choose_format(OutputFormat::Auto, false), OutputFormat::Jpeg);
        assert_eq!(choose_format(OutputFormat::Jpeg, true), OutputFormat::Jpeg);
        assert_eq!(choose_format(OutputFormat::Png, false), OutputFormat::Png);
    }
}
