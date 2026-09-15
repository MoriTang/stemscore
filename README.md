# StemFlow

[简体中文说明](README_zh.md)

A local-first AI music stem separator for audio files and the stereo output currently playing on macOS.

> Formerly StemScore. The GitHub repository and product-facing name are now StemFlow. Local data directories retain the former name so existing model downloads and output access continue to work.

## Features

- **File separation:** import WAV, MP3, FLAC, M4A/AAC, or OGG and create drums, bass, other, and vocals WAV files.
- **Live separation:** record the macOS default stereo output while running streaming ONNX inference.
- **Live visualization:** rolling four-stem waveforms, an input meter, and a drum light driven by the separated drum envelope.
- **Stem playback:** play, pause, and seek every completed stem after a file job or recording.
- **Local processing:** audio is not uploaded, and the Rust audio path does not require Python or PyTorch.

## Desktop Quick Start

```bash
git clone https://github.com/MoriTang/stemflow.git
cd stemflow/desktop
npm install
npm run dev
```

macOS 14.6 or newer can capture system output directly without a virtual audio driver. Development also requires Rust 1.88+, Node.js, npm, and Xcode Command Line Tools.

The first separation downloads and verifies one approximately **106 MB** HS-TasNet ONNX model. File and live separation share the same cached model, so it is not downloaded again on later runs.

```bash
# Build the macOS app
npm run build -- --bundles app
```

See [desktop/README.md](desktop/README.md) for detailed usage, output structure, and current limitations.

## Data and Outputs

| Content | Location |
|---------|----------|
| ONNX model cache | `~/Library/Application Support/com.MoriTang.StemScore/models/` |
| Live recordings | `~/Music/StemScore Recordings/` |
| File separation | `~/Music/StemScore Separations/` |

These directories retain the former name for compatibility with existing models and outputs.

## Rust CLI

The default CLI uses the same Rust decoder, resampler, model cache, and ONNX
separator as the desktop app. Python and PyTorch are not installed or loaded.

```bash
cargo build --release --manifest-path cli/Cargo.toml
./cli/target/release/stemflow separate song.mp3

# Or install the command into ~/.cargo/bin
cargo install --path cli
```

The first fast separation downloads and verifies the shared model. Later runs
reuse the cached file.

```bash
# Choose an output directory
stemflow separate song.mp3 --output ./result

# Keep all four stems and add a mix without vocals
stemflow separate song.mp3 --solo vocals

# Inspect, download, or verify the shared model
stemflow model status
stemflow model download
stemflow model verify
```

Fast results are written to:

```text
output/stems/
├── drums.wav
├── bass.wav
├── other.wav
└── vocals.wav
```

`--solo vocals` preserves those four files and adds
`accompaniment-without-vocals.wav`; it never deletes generated stems.

## Optional High-quality Demucs Backend

Demucs remains available only when explicitly requested. Install it in a
separate virtual environment:

```bash
python3 -m venv .venv-demucs
.venv-demucs/bin/pip install -r backends/demucs/requirements.txt

stemflow separate song.mp3 \
  --quality high \
  --python .venv-demucs/bin/python \
  --model htdemucs
```

You can set `STEMFLOW_DEMUCS_PYTHON` instead of passing `--python` each time.
The default Rust build does not bundle this environment, Demucs weights, or a
Python interpreter. PyInstaller, MIDI transcription, and MusicXML generation
are no longer part of the project.

## Current Constraints

- The fast HS-TasNet model is fixed to four stereo stems at 44.1 kHz and
  prioritizes latency over Demucs-level offline quality.
- High-quality mode still requires an external Python/PyTorch environment.
- The desktop system-audio capture path currently requires macOS 14.6 or newer;
  Rust file separation is portable.
- Classical and orchestral instruments mostly fall into the `other` stem.
- Do not use StemFlow to bypass DRM or process audio you do not have permission
  to record.

## License

MIT
