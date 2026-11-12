//! The whole pipeline, in the exact SPEC 9.1 order. `sniff` runs first,
//! so a refused format never reaches the decoder.
//!
//! Fail closed (S-6): the output is written to the private folder, read
//! back and verified. If the check fails, the file is deleted and an error
//! is returned. The original is never used as a fallback.

use std::path::{Path, PathBuf};

use crate::decode::{Decoder, GlycinDecoder};
use crate::encode::{Encoder, JpegEncoder, OutputProfile, Pixels, PngEncoder};
use crate::output::{PrivateDir, neutral_name};
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
    pub input_bytes: u64,
    pub input_size: (u32, u32),
    pub output_bytes: u64,
    pub output_size: (u32, u32),
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
        OutputFormat::Webp | OutputFormat::Auto => Err(Error::Encode(
            "WebP output is not supported in this version",
        )),
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
    sniff::check_file(input)?;
    let input_bytes = std::fs::metadata(input)?.len();

    // 1. Decode in the sandbox.
    let decoded = decoder.decode(input, limits)?;
    let input_size = decoded.pixels.dimensions();
    // 2. Orientation, before the metadata is gone (FR-2).
    let image = orient::apply(decoded.pixels, decoded.orientation);
    // 3. and 4. Crop, then burn in the redactions (FR-5).
    let image = edit::apply(image, opts.crop, &opts.redactions)?;
    // 5. Convert to sRGB; the source profile is dropped here.
    let image = color::to_srgb(image, decoded.color.as_ref())?;
    // 6. Fit-to-size encode.
    let px = Pixels::new(image, OUTPUT_PROFILE);
    let format = choose_format(opts.format, px.has_alpha);
    let encoder = encoder_for(format)?;
    let fitted = fit::fit(&px, opts.target_bytes, opts.max_long_edge, encoder.as_ref())?;
    // 7. Write to the private folder with a neutral name.
    let name = output_name(input, opts, target, encoder.extension());
    let path = target.dir.write(&name, &fitted.bytes)?;
    // 8. Read back and verify. Fail closed (S-6).
    if let Err(e) = verify_file(&path) {
        std::fs::remove_file(&path)?;
        return Err(e);
    }
    Ok(Prepared {
        path,
        input_bytes,
        input_size,
        output_bytes: fitted.bytes.len() as u64,
        output_size: (fitted.width, fitted.height),
    })
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
    let stem = input.file_stem().and_then(|s| s.to_str());
    match stem {
        Some(stem) if opts.keep_name && !stem.is_empty() => format!("{stem}.{ext}"),
        _ => neutral_name(target.position, target.total, ext),
    }
}

fn verify_file(path: &Path) -> Result<(), Error> {
    let bytes = std::fs::read(path)?;
    verify::verify(&bytes, OUTPUT_PROFILE.map(OutputProfile::bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_keeps_alpha_as_png() {
        assert_eq!(choose_format(OutputFormat::Auto, true), OutputFormat::Png);
        assert_eq!(choose_format(OutputFormat::Auto, false), OutputFormat::Jpeg);
        assert_eq!(choose_format(OutputFormat::Jpeg, true), OutputFormat::Jpeg);
        assert_eq!(choose_format(OutputFormat::Png, false), OutputFormat::Png);
    }
}
