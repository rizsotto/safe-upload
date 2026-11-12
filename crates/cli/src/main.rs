//! `safe-upload`: prepare images for upload from the command line.
//!
//! Exit codes (M1 plan): 0 success, 1 any other error, 2 usage error,
//! 3 input format not supported in this build (HEIC/HEIF in M1).

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{CommandFactory, Parser, ValueEnum, error::ErrorKind};
use safe_upload_core::output::PrivateDir;
use safe_upload_core::{Error, Format, Options, OutputFormat, Prepared, Target, sniff};

const EXIT_ERROR: u8 = 1;
const EXIT_UNSUPPORTED: u8 = 3;

#[derive(Parser)]
#[command(
    name = "safe-upload",
    version,
    about = "Remove metadata from images and fit them to a file size"
)]
struct Cli {
    /// Images to prepare.
    #[arg(required = true, value_name = "IN")]
    inputs: Vec<PathBuf>,
    /// Largest output file size, for example 2MB, 500KB or 1.5MiB.
    #[arg(long, value_name = "SIZE", value_parser = parse_size)]
    max_size: u64,
    /// Output file, or an existing folder. A batch needs a folder. Without
    /// it, the files stay in the private temporary folder.
    #[arg(short, value_name = "OUT")]
    o: Option<PathBuf>,
    /// Largest width or height in pixels.
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u32).range(1..))]
    max_edge: Option<u32>,
    /// Output format.
    #[arg(long, value_name = "F", value_enum, default_value_t = FormatArg::Auto)]
    format: FormatArg,
}

#[derive(Clone, Copy, ValueEnum)]
enum FormatArg {
    /// PNG for images with transparency, else JPEG.
    Auto,
    Jpeg,
    Png,
}

fn parse_size(s: &str) -> Result<u64, String> {
    safe_upload_core::parse_size(s).map_err(|e| e.to_string())
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => ExitCode::from(code),
    }
}

/// Prints the error for input `n` (from 1) and returns the exit code.
/// Never prints the path (S-5).
fn fail(n: usize, e: &Error) -> u8 {
    match e {
        Error::UnsupportedFormat(Format::Heif) => {
            eprintln!(
                "Input {n} is a HEIC/HEIF image. This version of safe-upload cannot read \
                 HEIC yet. Convert it to JPEG or PNG first, then try again. On an iPhone, \
                 Settings > Camera > Formats > Most Compatible saves new photos as JPEG."
            );
            EXIT_UNSUPPORTED
        }
        Error::UnsupportedFormat(_) => {
            eprintln!("safe-upload: input {n}: {e}");
            EXIT_UNSUPPORTED
        }
        _ => {
            eprintln!("safe-upload: input {n}: {e}");
            EXIT_ERROR
        }
    }
}

fn run(cli: &Cli) -> Result<(), u8> {
    let out_dir = match &cli.o {
        Some(o) if o.is_dir() => Some(o.as_path()),
        Some(_) if cli.inputs.len() > 1 => Cli::command()
            .error(
                ErrorKind::ArgumentConflict,
                "-o must be an existing folder for more than one input",
            )
            .exit(),
        _ => None,
    };

    // Check every input before processing any, so nothing is written when
    // one is refused. A refused format wins over other errors.
    let mut first_error = None;
    for (i, input) in cli.inputs.iter().enumerate() {
        if let Err(e) = sniff::check_file(input) {
            let refused = matches!(e, Error::UnsupportedFormat(_));
            if refused || first_error.is_none() {
                first_error = Some((i + 1, e));
            }
            if refused {
                break;
            }
        }
    }
    if let Some((n, e)) = first_error {
        return Err(fail(n, &e));
    }

    let dir = PrivateDir::create().map_err(|e| {
        eprintln!("safe-upload: {e}");
        EXIT_ERROR
    })?;
    let result = prepare_all(cli, &dir).and_then(|prepared| {
        match &cli.o {
            Some(o) => deliver(&cli.inputs, &prepared, o, out_dir.is_some())?,
            None => {
                for p in &prepared {
                    println!("{}", p.path.display());
                }
            }
        }
        // The summary goes to stderr, so stdout stays a list of paths.
        for (i, p) in prepared.iter().enumerate() {
            eprintln!("Image {}: {}", i + 1, p.summary);
        }
        Ok(())
    });
    if result.is_err() || cli.o.is_some() {
        let _ = dir.remove();
    }
    result
}

fn prepare_all(cli: &Cli, dir: &PrivateDir) -> Result<Vec<Prepared>, u8> {
    let mut opts = Options::new(cli.max_size);
    opts.max_long_edge = cli.max_edge;
    opts.format = match cli.format {
        FormatArg::Auto => OutputFormat::Auto,
        FormatArg::Jpeg => OutputFormat::Jpeg,
        FormatArg::Png => OutputFormat::Png,
    };
    let total = cli.inputs.len();
    cli.inputs
        .iter()
        .enumerate()
        .map(|(i, input)| {
            let target = Target {
                dir,
                position: i + 1,
                total,
            };
            safe_upload_core::prepare(input, &opts, target).map_err(|e| fail(i + 1, &e))
        })
        .collect()
}

/// Copies the verified files to `-o`. Checks every destination first, so
/// an original is never overwritten (FR-7).
fn deliver(inputs: &[PathBuf], prepared: &[Prepared], o: &Path, is_dir: bool) -> Result<(), u8> {
    let dests: Vec<PathBuf> = prepared
        .iter()
        .map(|p| match is_dir {
            true => o.join(p.path.file_name().expect("prepare names its output")),
            false => o.to_path_buf(),
        })
        .collect();
    for dest in &dests {
        if inputs.iter().any(|input| same_file(input, dest)) {
            eprintln!("safe-upload: the output would replace an input file");
            return Err(EXIT_ERROR);
        }
    }
    for (p, dest) in prepared.iter().zip(&dests) {
        if let Err(e) = fs::copy(&p.path, dest) {
            eprintln!("safe-upload: cannot write the output: {e}");
            return Err(EXIT_ERROR);
        }
    }
    Ok(())
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (fs::metadata(a), fs::metadata(b)) {
        (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
        _ => false,
    }
}
