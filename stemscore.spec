# -*- mode: python ; coding: utf-8 -*-
"""PyInstaller spec for transcription pipeline — onedir."""

import os
import site
from pathlib import Path

ROOT = Path(SPECPATH)

# Locate demucs remote/ directory (PyInstaller doesn't auto-collect data files)
_demucs_remote_dir = None
for _sp in site.getsitepackages():
    _candidate = Path(_sp) / "demucs" / "remote"
    if _candidate.is_dir():
        _demucs_remote_dir = _candidate
        break

checkpoint = ROOT / "note_F1=0.9677_pedal_F1=0.9186.pth"
datas = []
if _demucs_remote_dir:
    # Bundle all files from demucs/remote/ individually
    for _f in _demucs_remote_dir.iterdir():
        if _f.is_file():
            datas.append((str(_f), "demucs/remote"))
if checkpoint.exists():
    datas.append((str(checkpoint), "."))

# Bundle lilypond for PDF generation
lily_dir = ROOT / "lilypond"
if lily_dir.exists():
    datas.append((str(lily_dir), "lilypond"))

a = Analysis(
    ["main.py"],
    pathex=[str(ROOT)],
    binaries=[],
    datas=datas,
    hiddenimports=[
        "torch", "torch._C", "torch.utils", "torch.serialization",
        "torch.cuda", "torch.cuda._utils",
        "torch.distributed",
        "torchaudio",
        "demucs", "demucs.apply", "demucs.pretrained", "demucs.audio",
        "demucs.htdemucs", "demucs.hdemucs", "demucs.states",
        "demucs.transformer", "demucs.utils",
        "piano_transcription_inference",
        "piano_transcription_inference.inference",
        "piano_transcription_inference.utilities",
        "piano_transcription_inference.models",
        "piano_transcription_inference.piano_vad",
        "piano_transcription_inference.config",
        "music21", "music21.converter", "music21.converter.subConverters",
        "music21.meter", "music21.key", "music21.stream",
        "music21.note", "music21.chord", "music21.midi", "music21.midi.translate",
        "librosa", "librosa.core.audio", "librosa.util",
        "resampy", "soundfile", "audioread",
        "numba", "numba.core", "llvmlite",
        "numpy", "numpy.core", "scipy", "scipy.signal",
        "tqdm", "certifi", "lazy_loader", "mido",
        "signal",
    ],
    hookspath=[],
    runtime_hooks=[],
    excludes=[
        "torchvision",
        "matplotlib.tests", "numpy.tests", "PIL.ImageQt",
    ],
)

pyz = PYZ(a.pure)

exe = EXE(
    pyz,
    a.scripts,
    exclude_binaries=True,
    name="stemscore",
    debug=False,
    strip=False,
    upx=True,
    console=True,
    disable_windowed_traceback=False,
    target_arch=None,
    codesign_identity=None,
    entitlements_file=None,
)

coll = COLLECT(
    exe,
    a.binaries,
    a.datas,
    strip=False,
    upx=True,
    upx_exclude=[],
    name="stemscore",
)
