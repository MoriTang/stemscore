#!/usr/bin/env python3
"""Rewrite render_sheet_music: single-process, instrument-aware, signal timeout."""
from pathlib import Path

target = Path('engine.py')
with open(target, 'r') as f:
    lines = f.readlines()

# Find block: _format_by_instrument through render_sheet_music, before run_pipeline
start = end = None
for i, line in enumerate(lines):
    if line.startswith('def _format_by_instrument('):
        start = i
    if line.startswith('def run_pipeline('):
        end = i
        break

new_code = '''def _quantise_and_clean(score):
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
            print(f"  \\u2713 {name}.musicxml")
            if make_pdf and pdf_path and pdf_path.exists():
                print(f"  \\u2713 {name}.pdf")
        except Timeout:
            print(f"  \\u26a0\\ufe0f {name} 制谱超时 ({timeout}s)")
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
                print(f"  \\u2713 {name}.musicxml (简化)")
            except Exception as e:
                print(f"  \\u2717 {name} 简化制谱失败: {e}")
                results.append({"name": name, "midi": midi_path,
                               "musicxml": None, "pdf": None})
                _signal.signal(_signal.SIGALRM, old_handler)
                continue
        except Exception as e:
            _signal.alarm(0)
            print(f"  \\u2717 {name} 制谱失败: {e}")
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

'''

new_lines = lines[:start] + [new_code] + lines[end:]
with open(target, 'w') as f:
    f.writelines(new_lines)
print(f'OK: replaced lines {start+1}-{end} ({end-start} lines)')
