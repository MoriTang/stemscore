# StemFlow Desktop

Local-first Tauri 2 app for separating audio files or recording the computer's
stereo output into drums, bass, other, and vocals. The interface has two
independent workflows: **File Separation** and **Live Separation**.

StemFlow was previously named StemScore. The bundle identifier, model cache,
and output folder names intentionally retain `StemScore` during the rename so
existing downloads, macOS permissions, and recordings keep working.

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
repository. File and live separation share this model cache.

## File Separation tab

Choose a WAV, MP3, FLAC, M4A/AAC, or OGG file and start separation. Processing
runs locally in Rust. When it finishes, drums, bass, other, and vocals appear
as independent players with play/pause and seek controls.

Results are written to `~/Music/StemScore Separations/<file>-<timestamp>/` as
`drums.wav`, `bass.wav`, `other.wav`, and `vocals.wav`. File separation does
not require System Audio Recording permission.

## Live Separation tab

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

## Current constraints

- The live model is stereo and fixed at 44.1 kHz. Other device rates are
  converted on the inference worker; the untouched original stays at the
  device's native rate.
- Live stems prioritize latency. Re-run `original.wav` with
  `stemflow separate --quality high` when a higher-quality Demucs export is
  needed.
- Local-file separation currently uses the same low-latency HS-TasNet model.
  The optional `--quality high` Demucs backend remains the higher-quality
  option for offline exports.
- The first implementation captures the selected default output mix. A later
  macOS-specific adapter can narrow the tap to one process such as Music.
- Do not use the app to bypass DRM or copy audio you do not have permission to
  record.

## Model attribution

The streaming model contract and weights used by StemFlow come from
[StemgenRT](https://github.com/sweetspotsoundsystem/stemgen-rt), used under the
MIT License. Its expected identity is recorded in `src-tauri/src/model.rs`.
