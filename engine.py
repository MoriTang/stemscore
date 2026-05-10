# -*- coding: utf-8 -*-
"""
Core processing engine: source separation, transcription, sheet music rendering.

Pipeline:
  1. Demucs HTDemucs 4-source separation → stems (WAV)
  2. basic-pitch polyphonic transcription → MIDI per stem
  3. music21 → MusicXML + PDF sheet music per stem
"""

from pathlib import Path
from typing import Optional


# ---------------------------------------------------------------------------
# 1. Source separation
# ---------------------------------------------------------------------------

def separate_audio(
    audio_path: Path,
    output_dir: Path,
    model_name: str = "htdemucs",
    solo_stem: str | None = None,
) -> list[Path]:
    """
    Run Demucs source separation on an audio file.

    If solo_stem is given (e.g. "vocals"), only that stem is kept as-is;
    all other stems are merged into a single "other.wav" (or
    "accompaniment.wav" when solo_stem == "other").

    Returns a list of paths to the separated stem WAV files.
    """
    import sys
    print("[分离] 导入依赖库 (numpy, torch, demucs, soundfile) ...", flush=True)
    import numpy as np
    import torch
    from demucs.apply import apply_model
    from demucs.pretrained import get_model
    from demucs.audio import AudioFile
    import soundfile as sf

    print(f"[分离] 加载 Demucs 模型 '{model_name}' ...")
    print("[分离] （首次运行将自动下载，约 80 MB，请耐心等待...）")
    try:
        model = get_model(name=model_name)
    except Exception as e:
        print(f"[分离] ✗ 模型加载失败: {e}")
        print("[分离] 提示：检查网络连接，模型需要从 torch hub 下载")
        raise
    model.cpu()
    model.eval()
    print("[分离] ✓ 模型就绪")

    if solo_stem is not None and solo_stem not in model.sources:
        raise ValueError(
            f"未知声部 '{solo_stem}'。模型 '{model_name}' 支持: {', '.join(model.sources)}"
        )

    print(f"[分离] 处理音频: {audio_path.name}")
    # Load audio
    wav = AudioFile(str(audio_path)).read(streams=0, samplerate=model.samplerate, channels=model.audio_channels)
    wav = wav.cpu()
    ref = wav.mean(0)
    wav = (wav - ref.mean()) / ref.std()

    # Apply model
    with torch.no_grad():
        sources = apply_model(model, wav[None], device="cpu", shifts=1, split=True, overlap=0.25)

    sources = sources[0]  # remove batch dim

    # Collect all audio arrays
    sr = model.samplerate
    audios: dict[str, np.ndarray] = {}
    for name_idx, name in enumerate(model.sources):
        audios[name] = sources[name_idx].cpu().numpy().T.copy()

    stem_dir = output_dir / "stems"
    stem_dir.mkdir(parents=True, exist_ok=True)

    stem_paths: list[Path] = []

    if solo_stem is not None:
        # ── Solo mode: one stem kept, rest merged ──
        solo_path = stem_dir / f"{solo_stem}.wav"
        sf.write(str(solo_path), audios[solo_stem], sr)
        stem_paths.append(solo_path)
        print(f"  ✓ {solo_stem}.wav (独奏轨)")

        # Merge all other stems
        merged = None
        for name in model.sources:
            if name == solo_stem:
                continue
            if merged is None:
                merged = audios[name].copy()
            else:
                merged += audios[name]

        if merged is not None:
            # Avoid name collision when solo_stem == "other"
            merged_name = "accompaniment" if solo_stem == "other" else "other"
            merged_path = stem_dir / f"{merged_name}.wav"
            sf.write(str(merged_path), merged, sr)
            stem_paths.append(merged_path)
            other_names = [n for n in model.sources if n != solo_stem]
            print(f"  ✓ {merged_name}.wav (合并: {', '.join(other_names)})")
    else:
        # ── Normal mode: write all stems ──
        for name in model.sources:
            out_path = stem_dir / f"{name}.wav"
            sf.write(str(out_path), audios[name], sr)
            stem_paths.append(out_path)
            print(f"  ✓ {name}.wav")

    return stem_paths


# ---------------------------------------------------------------------------
# 1.5 Silence detection — skip stems with no meaningful audio
# ---------------------------------------------------------------------------

def detect_active_stems(
    stem_paths: list[Path],
    *,
    rms_threshold: float = 0.001,
) -> tuple[list[Path], list[Path]]:
    """
    Split stems into active (has audio) and silent (near-zero energy).

    Uses RMS energy: stems below rms_threshold are considered silent.
    Default threshold (0.001 ≈ -60 dBFS) catches truly empty tracks.

    Returns (active_stems, silent_stems).
    """
    import soundfile as sf
    import numpy as np

    active: list[Path] = []
    silent: list[Path] = []

    for p in stem_paths:
        try:
            data, _ = sf.read(str(p), dtype='float32', always_2d=True)
            data = data[:, 0]  # first channel
            rms = float(np.sqrt(np.mean(data ** 2)))
            if rms >= rms_threshold:
                active.append(p)
                print(f"  ✓ {p.stem}  (RMS={rms:.4f})")
            else:
                silent.append(p)
                print(f"  ✗ {p.stem}  静音, 跳过 (RMS={rms:.6f})")
        except Exception:
            # Can't read? Assume active rather than dropping data
            active.append(p)

    return active, silent


# ---------------------------------------------------------------------------
# 2. Audio → MIDI transcription
# ---------------------------------------------------------------------------

def transcribe_stems(
    stem_paths: list[Path],
    output_dir: Path,
    onset_threshold: float = 0.5,
    frame_threshold: float = 0.3,
    minimum_note_length: float = 58.0,
    checkpoint_path: str | None = None,
) -> list[Path]:
    """
    Transcribe each stem WAV to a MIDI file using piano-transcription-inference.

    piano-transcription-inference is a PyTorch-based polyphonic transcription model
    from the same research group as Demucs. It handles polyphonic audio well.
    """
    import sys as _sys
    print("[转写] 导入转录依赖 (piano_transcription_inference, basic_pitch) ...", flush=True)
    from piano_transcription_inference import PianoTranscription, sample_rate, load_audio

    midi_dir = output_dir / "midi"
    midi_dir.mkdir(parents=True, exist_ok=True)

    # Use CPU (MPS/GPU would need additional config)
    print(f"[转写] 加载转录模型...")
    if checkpoint_path:
        print(f"[转写] 检查点: {checkpoint_path}")
    else:
        print("[转写] （首次运行可能下载检查点，约 165 MB，请耐心等待...）")
    try:
        transcriptor = PianoTranscription(device="cpu", checkpoint_path=checkpoint_path)
    except Exception as e:
        print(f"[转写] ✗ 转录模型加载失败: {e}")
        print("[转写] 提示：请先运行 python3 download_checkpoint.py 下载检查点文件")
        raise
    print("[转写] ✓ 转录模型就绪")

    midi_paths: list[Path] = []
    for stem_path in stem_paths:
        name = stem_path.stem
        print(f"[转写] {name} ...")

        out_path = midi_dir / f"{name}.mid"

        # Load and resample audio
        (audio, _) = load_audio(str(stem_path), sr=sample_rate, mono=True)

        # Transcribe to MIDI
        transcriptor.transcribe(audio, str(out_path))
        midi_paths.append(out_path)
        print(f"  ✓ {name}.mid")

    return midi_paths


# ---------------------------------------------------------------------------
# 3. MIDI → sheet music (MusicXML + PDF)
# ---------------------------------------------------------------------------

def _quantise_and_clean(score):
    """Basic quantise: 16th-note rounding only."""
    parts = list(score.parts) if score.hasPartLikeStreams() else [score]
    for part in parts:
        for n in part.flatten().notesAndRests:
            try:
                raw = n.quarterLength
                n.quarterLength = max(0.25, round(raw * 4) / 4)
            except Exception:
                pass
    return score


def _format_by_instrument(score, stem_name: str):
    """Apply instrument-specific clef, staff, and transposition."""
    from music21 import clef, instrument, pitch

    settings = {
        "drums": {
            "clef": clef.PercussionClef(),
            "instrument": instrument.UnpitchedPercussion(),
            "transpose": None,
            "grand_staff": False,
        },
        "bass": {
            "clef": clef.BassClef(),
            "instrument": instrument.ElectricBass(),
            "transpose": -12,
            "grand_staff": False,
        },
        "piano": {
            "clef": None,
            "instrument": instrument.Piano(),
            "transpose": None,
            "grand_staff": True,
        },
        "guitar": {
            "clef": clef.Treble8vbClef(),
            "instrument": instrument.AcousticGuitar(),
            "transpose": None,
            "grand_staff": False,
        },
        "vocals": {
            "clef": clef.TrebleClef(),
            "instrument": instrument.Vocalist(),
            "transpose": None,
            "grand_staff": False,
        },
    }

    cfg = settings.get(stem_name)
    if cfg is None:
        return score

    parts = list(score.parts) if score.hasPartLikeStreams() else [score]

    for part in parts:
        try:
            if cfg["instrument"]:
                part.insert(0, cfg["instrument"])
        except Exception:
            pass
        try:
            if cfg["clef"] and not cfg["grand_staff"]:
                part.insert(0, cfg["clef"])
        except Exception:
            pass
        try:
            if cfg["transpose"] is not None:
                part.transpose(cfg["transpose"], inPlace=True)
        except Exception:
            pass

    if cfg.get("grand_staff"):
        try:
            from music21 import stream
            treble = stream.Part()
            bass_part = stream.Part()
            split_pitch = pitch.Pitch('C4').midi
            for el in score.flatten().notesAndRests:
                if hasattr(el, 'isNote') and el.isNote:
                    target = treble if el.pitch.midi >= split_pitch else bass_part
                    target.append(el)
                elif hasattr(el, 'isChord') and el.isChord:
                    avg = sum(p.midi for p in el.pitches) / len(el.pitches)
                    target = treble if avg >= split_pitch else bass_part
                    target.append(el)
                else:
                    treble.append(el)
            treble.insert(0, clef.TrebleClef())
            bass_part.insert(0, clef.BassClef())
            score = stream.Score()
            score.insert(0, bass_part)
            score.insert(0, treble)
        except Exception:
            pass

    return score


def _render_one_stem(midi_path: Path, xml_path: Path, pdf_path: Path | None,
                     lily_bin: str | None, stem_name: str = "other"):
    """Render a single stem in-process. Called with signal timeout from render_sheet_music."""
    from music21 import converter, environment
    import os as _os

    if lily_bin:
        env = environment.Environment()
        env["lilypondPath"] = lily_bin
        _os.environ["PATH"] = str(Path(lily_bin).parent) + _os.pathsep + _os.environ.get("PATH", "")

    score = converter.parse(str(midi_path))
    score = _format_by_instrument(score, stem_name)
    score = _quantise_and_clean(score)
    score.write("musicxml", str(xml_path))
    if pdf_path is not None:
        score.write("lily.pdf", str(pdf_path.with_suffix("")))


def render_sheet_music(
    midi_paths: list[Path],
    output_dir: Path,
    *,
    make_pdf: bool = True,
    timeout: int = 30,
) -> list[dict]:
    """Convert each MIDI file to MusicXML + optionally PDF.
    
    Uses signal-based timeout per stem. Applies instrument-specific formatting.
    """
    import sys as _sys
    print("[乐谱] 导入 music21 (首次加载约 5-10 秒) ...", flush=True)
    from music21 import environment
    import signal as _signal
    import os as _os

    _lily_bin = Path(__file__).resolve().parent / "lilypond" / "bin" / "lilypond"
    lily_bin_str = str(_lily_bin) if _lily_bin.exists() else None
    if lily_bin_str:
        env = environment.Environment()
        env["lilypondPath"] = lily_bin_str
        _os.environ["PATH"] = str(_lily_bin.parent) + _os.pathsep + _os.environ.get("PATH", "")

    xml_dir = output_dir / "musicxml"
    xml_dir.mkdir(parents=True, exist_ok=True)
    pdf_dir = output_dir / "pdf"
    pdf_dir.mkdir(parents=True, exist_ok=True)

    results: list[dict] = []

    class Timeout(Exception):
        pass

    for midi_path in midi_paths:
        name = midi_path.stem
        print(f"[制谱] {name} ...")

        xml_path = xml_dir / f"{name}.musicxml"
        pdf_path = pdf_dir / f"{name}.pdf" if make_pdf else None

        def _handler(signum, frame):
            raise Timeout()

        old_handler = _signal.signal(_signal.SIGALRM, _handler)
        _signal.alarm(timeout)
        try:
            _render_one_stem(midi_path, xml_path, pdf_path, lily_bin_str, name)
            _signal.alarm(0)
            print(f"  \u2713 {name}.musicxml")
            if make_pdf and pdf_path and pdf_path.exists():
                print(f"  \u2713 {name}.pdf")
        except Timeout:
            print(f"  \u26a0\ufe0f {name} 制谱超时 ({timeout}s)")
            _signal.alarm(0)  # already raised, just cleanup
            # Simplified fallback
            try:
                from music21 import converter
                score = converter.parse(str(midi_path))
                score = _format_by_instrument(score, name)
                for n in score.flatten().notesAndRests:
                    try:
                        raw = n.quarterLength
                        n.quarterLength = max(0.25, round(raw * 4) / 4)
                    except Exception:
                        pass
                score.write("musicxml", str(xml_path))
                print(f"  \u2713 {name}.musicxml (简化)")
            except Exception as e:
                print(f"  \u2717 {name} 简化制谱失败: {e}")
                results.append({"name": name, "midi": midi_path,
                               "musicxml": None, "pdf": None})
                _signal.signal(_signal.SIGALRM, old_handler)
                continue
        except Exception as e:
            _signal.alarm(0)
            print(f"  \u2717 {name} 制谱失败: {e}")
            results.append({"name": name, "midi": midi_path,
                           "musicxml": None, "pdf": None})
            _signal.signal(_signal.SIGALRM, old_handler)
            continue
        finally:
            _signal.signal(_signal.SIGALRM, old_handler)

        results.append({
            "name": name,
            "midi": midi_path,
            "musicxml": xml_path if xml_path.exists() else None,
            "pdf": pdf_path if (pdf_path and pdf_path.exists()) else None,
        })

    return results

def run_pipeline(
    audio_path: Path,
    output_dir: Path,
    *,
    demucs_model: str = "htdemucs",
    onset_threshold: float = 0.5,
    frame_threshold: float = 0.3,
    minimum_note_length: float = 58.0,
    skip_pdf: bool = False,
    skip_separation: bool = False,
    skip_transcribe: bool = False,
    checkpoint_path: str | None = None,
    output_midi: bool = False,
    silence_threshold: float = 0.001,
    solo_stem: str | None = None,
) -> dict:
    """
    Full pipeline: separate → transcribe → render sheet music.

    Set skip_separation=True to use existing stems/ directory.
    Set skip_transcribe=True to use existing midi/ directory.
    Set output_midi=True to keep MIDI files after sheet music generation.
    silence_threshold: RMS below which a stem is silent (default 0.001).
    solo_stem: if set, keep only that stem; merge rest into other.wav.

    Returns a summary dict with all output paths.
    """
    output_dir.mkdir(parents=True, exist_ok=True)

    # ── Resolve checkpoint path ──
    if checkpoint_path is None:
        import sys
        default_ckpt = Path.home() / "piano_transcription_inference_data" / "note_F1=0.9677_pedal_F1=0.9186.pth"
        workspace_ckpt = Path(__file__).parent / "note_F1=0.9677_pedal_F1=0.9186.pth"
        meipass_ckpt = Path(getattr(sys, "_MEIPASS", "")) / "note_F1=0.9677_pedal_F1=0.9186.pth"
        for ckpt in (default_ckpt, workspace_ckpt, meipass_ckpt):
            if ckpt.exists():
                checkpoint_path = str(ckpt)
                break

        # Fallback: download checkpoint with progress output
        if checkpoint_path is None:
            print("\n[检查点] 未找到检查点文件，开始下载...")
            print("[检查点] 来源: Zenodo (约 165 MB)")
            try:
                import requests
                url = "https://zenodo.org/record/4034264/files/CRNN_note_F1%3D0.9677_pedal_F1%3D0.9186.pth?download=1"
                dst = workspace_ckpt
                dst.parent.mkdir(parents=True, exist_ok=True)
                r = requests.get(url, stream=True, timeout=120)
                r.raise_for_status()
                total = int(r.headers.get("content-length", 0))
                downloaded = 0
                mode = "wb"
                for chunk in r.iter_content(chunk_size=65536):
                    with open(dst, mode) as f:
                        f.write(chunk)
                    mode = "ab"
                    downloaded += len(chunk)
                    if total:
                        pct = downloaded * 100 // total
                        print(f"\r[检查点] 下载中... {pct}% ({downloaded//1048576}/{total//1048576} MB)", end="", flush=True)
                print(f"\n[检查点] ✓ 下载完成: {dst}")
                checkpoint_path = str(dst)
            except Exception as e:
                print(f"\n[检查点] ✗ 下载失败: {e}")
                print("[检查点] 请手动运行: python3 download_checkpoint.py")
                raise RuntimeError(f"无法获取检查点文件，请先运行 download_checkpoint.py") from e

    # ── Step 1: Source separation ──
    stem_dir = output_dir / "stems"
    silent_stems: list[Path] = []
    if skip_separation:
        if stem_dir.exists():
            stem_paths = sorted(stem_dir.glob("*.wav"))
            print(f"[分离] 跳过, 使用已有 stems/ 目录 ({len(stem_paths)} 个文件)")
        else:
            print(f"\n[分离] ⚠️  --skip-separation 已指定, 但目录不存在:")
            print(f"          {stem_dir}")
            print(f"[分离] 回退执行音源分离...")
            print("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━")
            print(" 步骤 1/3: 音源分离 (Demucs)")
            print("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━")
            stem_paths = separate_audio(audio_path, output_dir,
                                        model_name=demucs_model, solo_stem=solo_stem)
    else:
        print("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━")
        print(" 步骤 1/3: 音源分离 (Demucs)")
        print("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━")
        stem_paths = separate_audio(audio_path, output_dir,
                                    model_name=demucs_model, solo_stem=solo_stem)

    # ── Silence detection ──
    print("\n[检测] 静音识别 ...")
    stem_paths, silent_stems = detect_active_stems(stem_paths, rms_threshold=silence_threshold)
    if silent_stems:
        names = ", ".join(p.stem for p in silent_stems)
        print(f"  跳过 {len(silent_stems)} 个静音轨: {names}")
    else:
        print(f"  所有 {len(stem_paths)} 轨均有有效音频")

    if not stem_paths:
        print("  ⚠️ 所有音轨均为静音, 跳过转录和制谱")
        return {"stems": [], "midi": [], "sheets": [], "silent_stems": silent_stems}

    # ── Step 2 & 3: Transcription + Sheet music (only when --midi) ──
    if not output_midi:
        print("\n[跳过] 转录和制谱 (使用 --midi 可开启)")
        return {
            "stems": stem_paths,
            "midi": [],
            "sheets": [],
            "silent_stems": silent_stems,
        }

    if output_midi:
        midi_dir = output_dir / "midi"
        keep_midi = True

    if skip_transcribe and (output_dir / "midi").exists():
        midi_paths = sorted((output_dir / "midi").glob("*.mid"))
        print(f"[转写] 跳过, 使用已有 midi/ 目录 ({len(midi_paths)} 个文件)")
    else:
        print("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━")
        print(" 步骤 2/3: 音频转 MIDI")
        print("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━")
        midi_paths = transcribe_stems(
            stem_paths, midi_dir,
            onset_threshold=onset_threshold,
            frame_threshold=frame_threshold,
            minimum_note_length=minimum_note_length,
            checkpoint_path=checkpoint_path,
        )

    # ── Step 3: Sheet music rendering ──
    print("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━")
    print(" 步骤 3/3: 生成乐谱 (music21)")
    print("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━")
    sheets = render_sheet_music(midi_paths, output_dir, make_pdf=not skip_pdf)

    return {
        "stems": stem_paths,
        "midi": midi_paths,
        "sheets": sheets,
        "silent_stems": silent_stems,
    }
