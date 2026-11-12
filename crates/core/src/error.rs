/// All errors from the core. Messages never contain metadata values or
/// file paths (S-5).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("invalid size: {0}")]
    InvalidSize(&'static str),
    #[error("the image has {pixels} pixels, the limit is {limit}")]
    TooManyPixels { pixels: u64, limit: u64 },
    #[error("the file has {bytes} bytes, the limit is {limit}")]
    FileTooLarge { bytes: u64, limit: u64 },
    #[error("{0} images are not supported in this version")]
    UnsupportedFormat(crate::Format),
    #[error("XDG_RUNTIME_DIR is not set or not usable")]
    NoRuntimeDir,
    #[error("the private temporary folder is not safe to use")]
    UnsafeTempDir,
    #[error("cannot decode the image: {0}")]
    Decode(&'static str),
    /// The sandboxed decoder could not run, so no file was decoded. Not a
    /// problem with the image: the system setup is broken (S-4, S-6).
    #[error("the image decoder could not run in its sandbox: {0}")]
    Sandbox(&'static str),
    #[error("color conversion failed: {0}")]
    Color(&'static str),
    #[error("invalid edit: {0}")]
    InvalidEdit(&'static str),
    #[error("encoding failed: {0}")]
    Encode(&'static str),
    #[error("the image cannot be made small enough for {target_bytes} bytes")]
    CannotFit { target_bytes: u64 },
    #[error("output check failed: {0}")]
    Verify(String),
    #[error("input/output error: {0}")]
    Io(#[from] std::io::Error),
}
