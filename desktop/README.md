# StemScore Live Desktop

Tauri 2 desktop app for separating local audio files or recording the
computer's stereo output into drums, bass, other, and vocals.

## Requirements

- macOS 14.6 or newer for native Core Audio loopback capture
- Rust 1.88 or newer
- Node.js and npm
- Xcode Command Line Tools

The capture path uses CPAL's output-device loopback support. On macOS this
creates a private Core Audio process tap and aggregate device; no virtual audio
driver is required.

## Run

```bash
cd desktop
npm install
npm run dev
```

On the first start, choose **Download model**. The app downloads the 106 MB
HS-TasNet ONNX file into the user application-data directory and verifies its
size and SHA-256 before installing it. The model is never committed to this
repository.

The first recording triggers macOS's System Audio Recording permission prompt.
Play music through the default stereo output device, then press **Start**.
The four stem cards display rolling min/max waveforms for approximately the
last eight seconds of separated audio; the final view remains after stopping.
The header's drum light follows the separated drum envelope with a fast attack
and smooth decay, and stays off when model separation is disabled.
After recording stops, each completed stem appears in the results player with
play/pause and seek controls. Starting one stem automatically pauses another.
Recordings are written to `~/Music/StemScore Recordings/`:

```text
recording-<timestamp>/
├── original.wav
└── stems/
    ├── drums.wav
    ├── bass.wav
    ├── other.wav
    └── vocals.wav
```

Use **record original only** to test system-audio capture without downloading
or loading ONNX Runtime.

For an existing audio file, use **Choose file** and **Start file separation**.
WAV, MP3, FLAC, M4A/AAC, and OGG are decoded directly in Rust. Results are
written to `~/Music/StemScore Separations/<file>-<timestamp>/` as `drums.wav`,
`bass.wav`, `other.wav`, and `vocals.wav`. File separation works independently
of system-audio recording permissions. The same four-track player appears when
file separation finishes.

## Current constraints

- The live model is stereo and fixed at 44.1 kHz. Other device rates are
  converted on the inference worker; the untouched original stays at the
  device's native rate.
- Live stems prioritize latency. Re-run `original.wav` through the Python
  Demucs pipeline when a higher-quality final export is needed.
- Local-file separation currently uses the same low-latency HS-TasNet model.
  The Python Demucs CLI remains the higher-quality option for offline exports.
- The first implementation captures the selected default output mix. A later
  macOS-specific adapter can narrow the tap to one process such as Music.
- Do not use the app to bypass DRM or copy audio you do not have permission to
  record.

## Model attribution

The streaming model contract and weights come from
[StemgenRT](https://github.com/sweetspotsoundsystem/stemgen-rt), used under the
MIT License. Its expected identity is recorded in `src-tauri/src/model.rs`.
