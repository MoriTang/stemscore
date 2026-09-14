import sys
from pathlib import Path


def fail(message):
    print(f"Demucs backend error: {message}", file=sys.stderr)
    raise SystemExit(2)


def main():
    if len(sys.argv) != 5:
        fail("expected INPUT OUTPUT MODEL EXCLUDED_STEM")

    input_path = Path(sys.argv[1])
    output_dir = Path(sys.argv[2])
    model_name = sys.argv[3]
    excluded_stem = sys.argv[4] or None

    try:
        import numpy as np
        import soundfile as sf
        import torch
        from demucs.apply import apply_model
        from demucs.audio import AudioFile
        from demucs.pretrained import get_model
    except ImportError as error:
        fail(
            f"missing optional Python dependency ({error.name}). "
            "Install it in a virtual environment with: "
            'python3 -m pip install "demucs>=4,<5" "soundfile>=0.12"'
        )

    model = get_model(name=model_name)
    model.cpu()
    model.eval()
    if excluded_stem is not None and excluded_stem not in model.sources:
        fail(
            f"model {model_name} does not provide {excluded_stem}; "
            f"available stems: {', '.join(model.sources)}"
        )

    waveform = AudioFile(str(input_path)).read(
        streams=0,
        samplerate=model.samplerate,
        channels=model.audio_channels,
    ).cpu()
    reference = waveform.mean(0)
    center = reference.mean()
    scale = reference.std()
    if float(scale) > 1e-8:
        waveform = (waveform - center) / scale
    else:
        scale = torch.tensor(1.0)
        center = torch.tensor(0.0)

    print(f"Loading Demucs model {model_name}...", flush=True)
    with torch.no_grad():
        sources = apply_model(
            model,
            waveform[None],
            device="cpu",
            shifts=1,
            split=True,
            overlap=0.25,
            progress=True,
        )[0]
    sources = sources * scale + center

    output_dir.mkdir(parents=True, exist_ok=True)
    audios = {}
    for index, name in enumerate(model.sources):
        audio = sources[index].cpu().numpy().T.copy()
        audios[name] = audio
        path = output_dir / f"{name}.wav"
        sf.write(path, audio, model.samplerate)
        print(f"  {path}")

    if excluded_stem is not None:
        accompaniment = np.zeros_like(next(iter(audios.values())))
        for name, audio in audios.items():
            if name != excluded_stem:
                accompaniment += audio
        path = output_dir / f"accompaniment-without-{excluded_stem}.wav"
        sf.write(path, accompaniment, model.samplerate)
        print(f"  {path}")


if __name__ == "__main__":
    main()
