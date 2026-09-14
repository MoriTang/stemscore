use std::{
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use stemflow_core::{file_separator, model_store};

#[derive(Debug, Parser)]
#[command(
    name = "stemflow",
    version,
    about = "Local-first music stem separation",
    long_about = "Separate local audio into drums, bass, other, and vocals. Fast mode is pure Rust + ONNX; high-quality mode optionally uses an external Python Demucs environment."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Separate an audio file into WAV stems.
    Separate {
        /// Input WAV, MP3, FLAC, M4A/AAC, or OGG file.
        input: PathBuf,

        /// Output directory. Stems are written under OUTPUT/stems/.
        #[arg(short, long, default_value = "./output")]
        output: PathBuf,

        /// Fast uses the local ONNX model; high invokes optional Python Demucs.
        #[arg(long, value_enum, default_value_t = Quality::Fast)]
        quality: Quality,

        /// Demucs model used by --quality high.
        #[arg(short, long, value_enum, default_value_t = DemucsModel::Htdemucs)]
        model: DemucsModel,

        /// Preserve all generated stems and add an accompaniment mix without this stem.
        #[arg(long, value_enum)]
        solo: Option<Stem>,

        /// Override the fast ONNX model path.
        #[arg(long)]
        model_path: Option<PathBuf>,

        /// Do not download a missing or invalid fast model.
        #[arg(long)]
        no_download: bool,

        /// Python executable for --quality high. Defaults to STEMFLOW_DEMUCS_PYTHON or python3.
        #[arg(long)]
        python: Option<PathBuf>,
    },

    /// Inspect or download the shared fast separation model.
    Model {
        #[command(subcommand)]
        command: ModelCommand,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
enum Quality {
    Fast,
    High,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Stem {
    Drums,
    Bass,
    Other,
    Vocals,
}

impl Stem {
    fn as_str(self) -> &'static str {
        match self {
            Self::Drums => "drums",
            Self::Bass => "bass",
            Self::Other => "other",
            Self::Vocals => "vocals",
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
enum DemucsModel {
    #[value(name = "htdemucs")]
    Htdemucs,
    #[value(name = "htdemucs_ft")]
    HtdemucsFt,
    #[value(name = "hdemucs_mmi")]
    HdemucsMmi,
    #[value(name = "htdemucs_6s")]
    Htdemucs6s,
}

impl DemucsModel {
    fn as_str(self) -> &'static str {
        match self {
            Self::Htdemucs => "htdemucs",
            Self::HtdemucsFt => "htdemucs_ft",
            Self::HdemucsMmi => "hdemucs_mmi",
            Self::Htdemucs6s => "htdemucs_6s",
        }
    }
}

#[derive(Debug, Subcommand)]
enum ModelCommand {
    /// Show the shared model path and validation status.
    Status,
    /// Download and verify the shared fast model.
    Download {
        /// Download even when a valid model is already installed.
        #[arg(long)]
        force: bool,
    },
    /// Verify the size and SHA-256 of the installed model.
    Verify,
}

fn main() {
    if let Err(error) = run(Cli::parse()) {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Commands::Separate {
            input,
            output,
            quality,
            model,
            solo,
            model_path,
            no_download,
            python,
        } => {
            validate_input(&input)?;
            let stem_dir = output.join("stems");
            match quality {
                Quality::Fast => {
                    if model != DemucsModel::Htdemucs {
                        bail!("--model only applies to --quality high");
                    }
                    let path = model_path.unwrap_or(model_store::default_model_path()?);
                    ensure_fast_model(&path, no_download)?;
                    run_fast(&input, &stem_dir, &path, solo)?;
                }
                Quality::High => {
                    if model_path.is_some() || no_download {
                        bail!("--model-path and --no-download only apply to --quality fast");
                    }
                    run_high(&input, &stem_dir, model, solo, python.as_deref())?;
                }
            }
            print_outputs(&stem_dir)?;
        }
        Commands::Model { command } => run_model_command(command)?,
    }
    Ok(())
}

fn validate_input(input: &Path) -> Result<()> {
    if !input.is_file() {
        bail!("audio file does not exist: {}", input.display());
    }
    Ok(())
}

fn ensure_fast_model(path: &Path, no_download: bool) -> Result<()> {
    match model_store::verify(path) {
        Ok(()) => return Ok(()),
        Err(error) if no_download => {
            return Err(error)
                .with_context(|| format!("fast model is unavailable at {}", path.display()));
        }
        Err(error) => {
            if path.exists() {
                eprintln!("Cached model is invalid ({error}); downloading a verified copy.");
            } else {
                eprintln!("Fast model is not installed; downloading it once.");
            }
        }
    }
    download_model(path)
}

fn download_model(path: &Path) -> Result<()> {
    let mut last_percent = u64::MAX;
    model_store::download(path, |progress| {
        let percent = progress
            .downloaded
            .saturating_mul(100)
            .checked_div(progress.total)
            .unwrap_or(0);
        if percent != last_percent {
            eprint!("\rDownloading model: {percent:>3}%");
            let _ = io::stderr().flush();
            last_percent = percent;
        }
    })?;
    eprintln!("\rDownloading model: 100%");
    eprintln!("Model installed: {}", path.display());
    Ok(())
}

fn run_fast(input: &Path, stem_dir: &Path, model_path: &Path, solo: Option<Stem>) -> Result<()> {
    eprintln!("Separating with the fast Rust/ONNX backend...");
    let mut last_percent = u64::MAX;
    file_separator::separate_audio_file(input, stem_dir, model_path, |progress| {
        let Some(total) = progress.total_input_frames else {
            return;
        };
        let percent = progress
            .processed_input_frames
            .saturating_mul(100)
            .checked_div(total)
            .unwrap_or(0);
        if percent != last_percent {
            eprint!("\rSeparating: {percent:>3}%");
            let _ = io::stderr().flush();
            last_percent = percent;
        }
    })?;
    eprintln!("\rSeparating: 100%");
    if let Some(stem) = solo {
        let mix = file_separator::create_accompaniment_mix(stem_dir, stem.as_str())?;
        eprintln!("Added accompaniment mix: {}", mix.display());
    }
    Ok(())
}

fn run_high(
    input: &Path,
    stem_dir: &Path,
    model: DemucsModel,
    solo: Option<Stem>,
    python_override: Option<&Path>,
) -> Result<()> {
    let python = python_override
        .map(PathBuf::from)
        .or_else(|| env::var_os("STEMFLOW_DEMUCS_PYTHON").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("python3"));
    eprintln!(
        "Separating with optional Python Demucs backend ({})...",
        model.as_str()
    );
    let status = Command::new(&python)
        .arg("-c")
        .arg(include_str!("demucs_backend.py"))
        .arg(input)
        .arg(stem_dir)
        .arg(model.as_str())
        .arg(solo.map(Stem::as_str).unwrap_or(""))
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| format!("failed to start Python interpreter: {}", python.display()))?;
    if !status.success() {
        bail!(
            "Demucs backend failed with {status}. Install optional dependencies with: python3 -m pip install \"demucs>=4,<5\" \"soundfile>=0.12\""
        );
    }
    Ok(())
}

fn run_model_command(command: ModelCommand) -> Result<()> {
    let path = model_store::default_model_path()?;
    match command {
        ModelCommand::Status => {
            println!("Path: {}", path.display());
            if !model_store::is_installed(&path) {
                println!("Status: not installed");
            } else {
                match model_store::verify(&path) {
                    Ok(()) => println!("Status: installed and verified"),
                    Err(error) => println!("Status: invalid ({error})"),
                }
            }
        }
        ModelCommand::Download { force } => {
            if !force && model_store::verify(&path).is_ok() {
                println!(
                    "Model is already installed and verified: {}",
                    path.display()
                );
            } else {
                download_model(&path)?;
            }
        }
        ModelCommand::Verify => {
            model_store::verify(&path)?;
            println!("Model verified: {}", path.display());
        }
    }
    Ok(())
}

fn print_outputs(stem_dir: &Path) -> Result<()> {
    println!("Separation complete: {}", stem_dir.display());
    let mut outputs = fs::read_dir(stem_dir)
        .with_context(|| format!("failed to read output directory: {}", stem_dir.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|extension| extension == "wav"))
        .collect::<Vec<_>>();
    outputs.sort();
    for path in outputs {
        println!("  {}", path.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_fast_quality() {
        let cli = Cli::try_parse_from(["stemflow", "separate", "song.wav"]).unwrap();
        match cli.command {
            Commands::Separate { quality, .. } => assert_eq!(quality, Quality::Fast),
            _ => panic!("expected separate command"),
        }
    }

    #[test]
    fn accepts_high_quality_demucs_model() {
        let cli = Cli::try_parse_from([
            "stemflow",
            "separate",
            "song.wav",
            "--quality",
            "high",
            "--model",
            "htdemucs_6s",
        ])
        .unwrap();
        match cli.command {
            Commands::Separate { quality, model, .. } => {
                assert_eq!(quality, Quality::High);
                assert!(matches!(model, DemucsModel::Htdemucs6s));
            }
            _ => panic!("expected separate command"),
        }
    }
}
