# StemScore

Automatic music source separation, MIDI transcription, and sheet music generation.

```
song → 4~6 stems (WAV) → MIDI → MusicXML scores
```

## Quick Start

```bash
git clone <repo>
cd transcription
python3 -m venv venv
source venv/bin/activate
pip install -r requirements.txt

# Only needed for --midi: pre-download transcription checkpoint (~165 MB)
python3 download_checkpoint.py

# Separate stems only (default)
python3 main.py song.mp3

# Full pipeline: separate + transcribe + sheet music
python3 main.py song.mp3 --midi
```

## First Run: Downloads

The first run is slow because several dependencies are downloaded **once** and cached:

| Download | Size | When | Notes |
|----------|------|------|-------|
| `pip install -r requirements.txt` | ~2 GB | Setup | Mostly PyTorch (~1.5 GB); one-time per venv |
| Demucs model | ~80 MB | First actual run | Auto-downloaded from torch hub to `~/.cache/torch/` |
| Transcription checkpoint | ~165 MB | First `--midi` run that performs transcription | piano-transcription weights, downloaded on demand |

**In practice**: a cold start (fresh venv, no cache) takes 5–15 minutes depending on network. After that, all downloads are cached and subsequent runs complete in seconds to minutes (depending on audio length).

If MIDI output is needed, the transcription checkpoint can be downloaded ahead of time:
```bash
python3 download_checkpoint.py   # Pre-download checkpoint
```

## Usage

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
