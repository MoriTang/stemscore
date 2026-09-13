# StemFlow

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

## Python CLI (Retained for Compatibility)

The Demucs command-line pipeline remains available for offline jobs where quality matters more than latency. Optional `--midi` processing creates MIDI and MusicXML; PDF score generation has been removed.

```bash
cd stemflow
python3 -m venv venv
source venv/bin/activate
pip install -r requirements.txt
python3 main.py song.mp3
```

## Python CLI Usage

```bash
python3 main.py <audio_file> [options]
```

| Option | Description |
|--------|-------------|
| `-o DIR` | Output directory (default: `./output`) |
| `-m MODEL` | Separation model (default: `htdemucs`) |
| `--midi` | Enable transcription and sheet music |
| `--fast` | Fast mode: ~2x separation speed (slightly lower quality) |
| `--solo STEM` | Extract a single stem, merge rest into other.wav |
| `--skip-separation` | Skip separation, use existing stems/ |
| `--skip-transcribe` | Skip transcription, use existing midi/ |
| `--silence-threshold RMS` | Silence detection threshold (default: 0.001) |
| `-h` | Show all options |

### Examples

```bash
# Basic: separate 4 stems
python3 main.py song.mp3

# Full pipeline: stems + MIDI + sheet music
python3 main.py song.mp3 --midi

# Fast mode
python3 main.py song.mp3 --midi --fast

# Karaoke: extract vocals, merge rest into accompaniment
python3 main.py song.mp3 --solo vocals --midi

# 6-stem separation (experimental; guitar ok, piano has artifacts)
python3 main.py song.mp3 -m htdemucs_6s --midi

# Re-generate sheet music from existing stems + MIDI
python3 main.py song.mp3 --skip-separation --skip-transcribe --midi
```

## Output Structure

```
output/
├── stems/          # Separated WAV tracks
│   ├── bass.wav
│   ├── drums.wav
│   ├── other.wav
│   └── vocals.wav
├── midi/           # MIDI files (requires --midi)
└── musicxml/       # MusicXML scores (requires --midi)
```

## Models

| Model | Stems | Notes |
|-------|-------|-------|
| `htdemucs` | 4 | Default: drums, bass, other, vocals |
| `htdemucs_ft` | 4 | Fine-tuned, same sources |
| `hdemucs_mmi` | 4 | Multi-instrument trained, same sources |
| `htdemucs_6s` | 6 | Experimental: + guitar, piano |

## Instrument-Specific Formatting

Sheet music is automatically optimized per stem:

| Stem | Clef | Layout |
|------|------|--------|
| bass | Bass clef | Single staff |
| drums | Percussion clef | Rhythm notation |
| guitar | Treble 8vb clef | Single staff |
| piano | Grand staff | Treble + bass |
| vocals | Treble clef | Single staff |

MusicXML files can be opened directly in [MuseScore](https://musescore.org) (free).

## Building a Standalone Executable

```bash
./build.sh
# Output: dist/stemscore/stemscore
# Usage:  dist/stemscore/stemscore song.mp3 --midi
```

## Classical Music

Supported, but separation quality is lower — Demucs is trained on pop/rock. Most orchestral instruments end up in the `other` stem and cannot be split into individual parts. Silence detection automatically skips empty stems.

## License

MIT
