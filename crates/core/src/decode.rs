//! Decodes untrusted files with glycin inside a sandbox (SPEC 9.1 step 1,
//! S-4, NFR-2): bwrap on the host, `flatpak-spawn --sandbox` in a Flatpak.
//!
//! Before glycin sees a file we check its size, refuse HEIF (`sniff`),
//! accept only JPEG, PNG, WebP and AVIF, and check that the file is not
//! cut off: glycin fills in missing JPEG data without an error. After
//! `load()` and before `next_frame()` we check the pixel count, because
//! `next_frame()` allocates the full buffer.

use std::io::Read;
use std::path::Path;

use futures_lite::future::block_on;
use glycin::{Loader, MainContextSelector, MemoryFormat, MemoryFormatSelection, SandboxMechanism};
use image::RgbaImage;

use crate::color::SourceColor;
use crate::{Error, Limits, Orientation, sniff};

/// A decoded image, before orientation and color conversion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decoded {
    /// 8 bit RGBA, straight alpha, as stored (not yet oriented).
    pub pixels: RgbaImage,
    /// The source color description: the ICC profile if there is one,
    /// else CICP. Only for color conversion; never written to the output.
    pub color: Option<SourceColor>,
    pub orientation: Orientation,
}

pub trait Decoder {
    fn decode(&self, path: &Path, limits: &Limits) -> Result<Decoded, Error>;
}

/// The glycin decoder. Always uses a sandbox (see [`sandbox_for`]); if it
/// cannot run, decoding fails.
#[derive(Debug, Clone, Copy, Default)]
pub struct GlycinDecoder;

impl Decoder for GlycinDecoder {
    fn decode(&self, path: &Path, limits: &Limits) -> Result<Decoded, Error> {
        let file = std::fs::File::open(path)?;
        limits.check_file_size(file.metadata()?.len())?;
        let mut bytes = Vec::new();
        file.take(limits.max_file_bytes.saturating_add(1))
            .read_to_end(&mut bytes)?;
        limits.check_file_size(bytes.len() as u64)?;
        self.decode_bytes(bytes, limits)
    }
}

impl GlycinDecoder {
    pub fn decode_bytes(&self, bytes: Vec<u8>, limits: &Limits) -> Result<Decoded, Error> {
        let mut image = open(bytes, limits)?;
        let details = image.details();
        limits.check_dimensions(details.width(), details.height())?;
        let orientation = Orientation::from_exif(image.transformation_orientation() as u16);

        let frame = block_on(image.next_frame()).map_err(|e| load_error(&e, "decoding failed"))?;
        let (w, h) = (frame.width(), frame.height());
        // The pixel count was checked from the header, before the frame
        // was allocated.
        if (w, h) != (details.width(), details.height()) {
            return Err(Error::Decode("frame size differs from header"));
        }
        if frame.memory_format() != MemoryFormat::R8g8b8a8 {
            return Err(Error::Decode("unexpected pixel format"));
        }
        let row = w as usize * 4;
        let stride = frame.stride() as usize;
        let buf = frame.buf_slice();
        if w == 0 || h == 0 || stride < row || buf.len() < stride * (h as usize - 1) + row {
            return Err(Error::Decode("bad frame size"));
        }
        let mut data = Vec::with_capacity(row * h as usize);
        for line in buf.chunks(stride).take(h as usize) {
            data.extend_from_slice(&line[..row]);
        }
        let pixels = RgbaImage::from_raw(w, h, data).ok_or(Error::Decode("bad frame size"))?;
        let details = frame.details();
        let color = match (details.color_icc_profile(), details.color_cicp()) {
            (Some(icc), _) => Some(SourceColor::Icc(icc.to_vec())),
            (None, Some(cicp)) => Some(SourceColor::Cicp(cicp.to_bytes())),
            (None, None) => None,
        };
        Ok(Decoded {
            pixels,
            color,
            orientation,
        })
    }
}

const FLATPAK_INFO: &str = "/.flatpak-info";

/// The sandbox to decode in: `FlatpakSpawn` if `flatpak_info` exists (we
/// run in a Flatpak, where bwrap cannot run), else `Bwrap`. Never `Auto`,
/// which silently drops the sandbox when bwrap is blocked, and never
/// `NotSandboxed`.
fn sandbox_for(flatpak_info: &Path) -> SandboxMechanism {
    if flatpak_info.exists() {
        SandboxMechanism::FlatpakSpawn
    } else {
        SandboxMechanism::Bwrap
    }
}

/// Runs the checks before glycin, then `load()` in the sandbox.
fn open(bytes: Vec<u8>, limits: &Limits) -> Result<glycin::Image, Error> {
    sniff::sniff(&bytes)?;
    check_complete(&bytes)?;
    let sandbox = sandbox_for(Path::new(FLATPAK_INFO));
    let mut loader = Loader::new_vec(bytes);
    // Managed: glycin runs its own loop thread, so decoding never waits on
    // the app's main loop.
    loader
        .sandbox_selector(sandbox.into_selector())
        .main_context_selector(MainContextSelector::Managed)
        .apply_transformations(false)
        .color_convert_icc_srgb(false)
        .accepted_memory_formats(MemoryFormatSelection::R8g8b8a8)
        .limits(glycin::Limits::default().timeout(limits.decode_timeout));
    let image = block_on(loader.load()).map_err(|e| load_error(&e, "cannot read the image"))?;
    if image.active_sandbox_mechanism() != sandbox {
        return Err(Error::Sandbox("the decoder did not run in the sandbox"));
    }
    Ok(image)
}

/// Splits glycin errors into "the file is bad" ([`Error::Decode`] with
/// `bad_file`) and "the sandbox did not work" ([`Error::Sandbox`]). Only
/// errors the loader itself sent back from inside the sandbox, a format it
/// does not know, or a timeout blame the file. Everything else (spawn
/// failures, the loader dying early, D-Bus, seccomp, I/O) is a sandbox
/// failure. glycin's error kind is private, so its remote errors are known
/// by their `Display` prefix; `loader_errors_blame_the_file` guards that.
fn load_error(e: &glycin::Error, bad_file: &'static str) -> Error {
    if e.unsupported_format().is_some() {
        Error::Decode("unknown or unsupported image format")
    } else if e.is_timeout() {
        Error::Decode("decoding took too long")
    } else if e.to_string().starts_with("Remote error:") {
        Error::Decode(bad_file)
    } else {
        Error::Sandbox("the decoder failed to start or stopped early")
    }
}

/// Refuses formats we do not accept and files that are cut off.
fn check_complete(b: &[u8]) -> Result<(), Error> {
    let complete = if b.starts_with(&[0xff, 0xd8]) {
        jpeg_complete(b)
    } else if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        png_complete(b)
    } else if b.starts_with(b"RIFF") && b.get(8..12) == Some(b"WEBP") {
        let size = u32::from_le_bytes(b[4..8].try_into().unwrap());
        b.len() as u64 >= 8 + u64::from(size)
    } else if is_avif(b) {
        isobmff_complete(b)
    } else {
        return Err(Error::Decode("unknown or unsupported image format"));
    };
    if complete {
        Ok(())
    } else {
        Err(Error::Decode("the file is cut off"))
    }
}

/// Walks the JPEG markers up to EOI. Segments are skipped by their length,
/// so an embedded thumbnail's EOI does not count. Data after EOI is fine.
fn jpeg_complete(b: &[u8]) -> bool {
    let mut i = 2;
    loop {
        if b.get(i) != Some(&0xff) {
            return false;
        }
        while b.get(i) == Some(&0xff) {
            i += 1;
        }
        let Some(&marker) = b.get(i) else {
            return false;
        };
        i += 1;
        match marker {
            0xd9 => return true,
            0x01 | 0xd0..=0xd7 => continue,
            _ => {}
        }
        let Some(len) = b.get(i..i + 2) else {
            return false;
        };
        let len = usize::from(u16::from_be_bytes([len[0], len[1]]));
        if len < 2 || i + len > b.len() {
            return false;
        }
        i += len;
        if marker == 0xda {
            // Entropy-coded data runs to the next marker that is not a
            // stuffed byte or a restart marker.
            loop {
                match b.get(i..i + 2) {
                    None => return false,
                    Some([0xff, 0x00 | 0xd0..=0xd7 | 0xff]) => i += 1,
                    Some([0xff, _]) => break,
                    Some(_) => i += 1,
                }
            }
        }
    }
}

/// Walks the PNG chunks up to IEND.
fn png_complete(b: &[u8]) -> bool {
    let mut i = 8;
    while let Some(head) = b.get(i..i + 8) {
        let len = u32::from_be_bytes(head[0..4].try_into().unwrap()) as usize;
        let Some(end) = i.checked_add(len).and_then(|n| n.checked_add(12)) else {
            return false;
        };
        if end > b.len() {
            return false;
        }
        if &head[4..8] == b"IEND" {
            return true;
        }
        i = end;
    }
    false
}

/// An `ftyp` box with an `avif` or `avis` brand. Other HEIF brands were
/// refused by `sniff`.
fn is_avif(b: &[u8]) -> bool {
    if b.get(4..8) != Some(b"ftyp") {
        return false;
    }
    let size = u32::from_be_bytes(b[0..4].try_into().unwrap()) as usize;
    let Some(ftyp) = b.get(8..size.min(b.len())) else {
        return false;
    };
    // Major brand, minor version, then compatible brands.
    let brands = ftyp
        .get(..4)
        .into_iter()
        .chain(ftyp.get(8..).unwrap_or(&[]).chunks(4));
    brands
        .into_iter()
        .any(|brand| brand == b"avif" || brand == b"avis")
}

/// Every top-level box must fit in the file.
fn isobmff_complete(b: &[u8]) -> bool {
    let mut i = 0usize;
    while i < b.len() {
        let Some(head) = b.get(i..i + 8) else {
            return false;
        };
        let size = match u32::from_be_bytes(head[0..4].try_into().unwrap()) {
            0 => return true,
            1 => match b.get(i + 8..i + 16) {
                Some(large) => u64::from_be_bytes(large.try_into().unwrap()),
                None => return false,
            },
            n => u64::from(n),
        };
        match usize::try_from(size).ok().and_then(|s| i.checked_add(s)) {
            Some(end) if size >= 8 && end <= b.len() => i = end,
            _ => return false,
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encode::{Encoder, JpegEncoder, Pixels, PngEncoder};
    use image::Rgba;

    fn jpeg() -> Vec<u8> {
        let image = RgbaImage::from_pixel(32, 16, Rgba([200, 30, 30, 255]));
        JpegEncoder.encode(&Pixels::new(image, None), 90).unwrap()
    }

    #[test]
    fn core_decoder_runs_in_bwrap() {
        let image = open(jpeg(), &Limits::default()).unwrap();
        assert_eq!(image.active_sandbox_mechanism(), SandboxMechanism::Bwrap);
    }

    #[test]
    fn loader_errors_blame_the_file() {
        // Complete JPEG structure (SOI, COM, EOI) but no image in it. The
        // loader runs in the sandbox and sends back an error.
        let bad = vec![0xff, 0xd8, 0xff, 0xfe, 0x00, 0x04, b'h', b'i', 0xff, 0xd9];
        assert!(jpeg_complete(&bad));
        let err = GlycinDecoder.decode_bytes(bad, &Limits::default());
        assert!(matches!(err, Err(Error::Decode(_))), "{err:?}");
    }

    #[test]
    fn local_glycin_errors_are_sandbox_failures() {
        let io = glycin::Error::from(std::io::Error::other("spawn failed"));
        assert!(matches!(load_error(&io, "x"), Error::Sandbox(_)));
        let other = glycin::Error::other("loader exited early");
        assert!(matches!(load_error(&other, "x"), Error::Sandbox(_)));
    }

    #[test]
    fn flatpak_info_selects_flatpak_spawn() {
        let dir = std::env::temp_dir().join(format!("su-sandbox-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let info = dir.join(".flatpak-info");
        std::fs::write(&info, "[Application]\n").unwrap();
        assert_eq!(sandbox_for(&info), SandboxMechanism::FlatpakSpawn);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(sandbox_for(&info), SandboxMechanism::Bwrap);
    }

    #[test]
    fn decodes_bytes() {
        let d = GlycinDecoder
            .decode_bytes(jpeg(), &Limits::default())
            .unwrap();
        assert_eq!(d.pixels.dimensions(), (32, 16));
        assert!(d.color.is_none());
        assert_eq!(d.orientation, Orientation::Normal);
        let p = d.pixels.get_pixel(5, 5);
        assert!(p[0] > 180 && p[1] < 60 && p[3] == 255, "{p:?}");
    }

    #[test]
    fn complete_files_pass() {
        assert!(jpeg_complete(&jpeg()));
        let png = PngEncoder
            .encode(&Pixels::new(RgbaImage::new(4, 4), None), 100)
            .unwrap();
        assert!(png_complete(&png));
        let mut trailing = jpeg();
        trailing.extend_from_slice(b"trailer");
        assert!(jpeg_complete(&trailing));
    }

    #[test]
    fn every_cut_is_found() {
        let j = jpeg();
        for n in 0..j.len() {
            assert!(check_complete(&j[..n]).is_err(), "jpeg cut at {n}");
        }
        let png = PngEncoder
            .encode(&Pixels::new(RgbaImage::new(4, 4), None), 100)
            .unwrap();
        for n in 0..png.len() {
            assert!(check_complete(&png[..n]).is_err(), "png cut at {n}");
        }
    }

    #[test]
    fn webp_and_avif_size_checks() {
        let mut webp = b"RIFF\x0c\0\0\0WEBPVP8 ".to_vec();
        assert!(check_complete(&webp).is_err());
        webp.extend_from_slice(b"\0\0\0\0");
        assert!(check_complete(&webp).is_ok());

        let mut avif = b"\0\0\0\x14ftypavif\0\0\0\0avif".to_vec();
        avif.extend_from_slice(b"\0\0\0\x10mdat");
        assert!(check_complete(&avif).is_err());
        avif.extend_from_slice(&[0; 8]);
        assert!(check_complete(&avif).is_ok());
    }

    #[test]
    fn only_avif_brands_count_as_avif() {
        let mut mp4 = b"\0\0\0\x14ftypmp42\0\0\0\0isom".to_vec();
        mp4.extend_from_slice(b"\0\0\0\x08free");
        assert!(matches!(check_complete(&mp4), Err(Error::Decode(_))));
        let mut avis = b"\0\0\0\x14ftypmif1\0\0\0\0avis".to_vec();
        avis.extend_from_slice(b"\0\0\0\x08free");
        assert!(check_complete(&avis).is_ok());
    }

    #[test]
    fn unknown_formats_are_refused() {
        for b in [&b"GIF89a......"[..], b"<svg xmlns=", b"BM", b""] {
            assert!(matches!(check_complete(b), Err(Error::Decode(_))));
        }
    }
}
