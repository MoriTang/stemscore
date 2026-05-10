#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
StemScore — 从音频文件自动生成多乐器分谱

Usage:
    python main.py <音频文件> [-o <输出目录>] [选项]

Example:
    python main.py orchestra.mp3 -o ./output
    python main.py quartet.wav --no-pdf --model htdemucs_ft

Output structure:
    <output_dir>/
    ├── stems/          # 分离后的音轨 (WAV)
    │   ├── drums.wav
    │   ├── bass.wav
    │   ├── other.wav
    │   └── vocals.wav
    ├── midi/           # 各轨的 MIDI 文件
    │   ├── drums.mid
    │   └── ...
    ├── musicxml/       # MusicXML 乐谱 (可导入 MuseScore 等)
    │   └── ...
    └── pdf/            # PDF 五线谱 (需要 Lilypond 或 MuseScore)
        └── ...
"""

import argparse
import os
import sys
import textwrap
from pathlib import Path

# Fix SSL certificate issues on macOS with misconfigured cert chains
try:
    import certifi
    os.environ["SSL_CERT_FILE"] = certifi.where()
except ImportError:
    pass

from engine import run_pipeline


def main():
    parser = argparse.ArgumentParser(
        description="从音频文件中分离乐器并生成分谱 (PDF + MIDI)",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=textwrap.dedent("""\
        DEMUCS 模型选择:
          htdemucs        - 默认 4 声源: drums, bass, other, vocals
          htdemucs_ft     - 微调版 4 声源: drums, bass, other, vocals
          hdemucs_mmi     - 多乐器训练, 相同 4 声源但权重不同
          htdemucs_6s     - 实验性 6 声源: + guitar, piano (guitar 尚可, piano 有杂音)

        输出说明:
          - 默认仅分离音轨: stems/ (WAV), 不转录不制谱
          - 使用 --midi 开启转录和制谱: + midi/ + musicxml/ + pdf/
          - 使用 --no-pdf 可跳过 PDF 生成
          - 使用 --solo <声部> 可仅提取指定声部, 其余合并为 other.wav

        单轨提取示例:
          python main.py song.mp3 --solo vocals    # 人声 (卡拉OK模式)
          python main.py song.mp3 --solo bass      # 贝斯 + 其余
          python main.py song.mp3 --solo drums     # 鼓 + 其余

        注意事项:
          - 首次运行会下载模型 (~350MB), 请确保网络畅通
          - PDF 生成需要安装 Lilypond (https://lilypond.org) 或 MuseScore
          - 古典管弦乐的分离精度受限于模型训练数据, 主要集中在流行/摇滚乐器
        """),
    )

    parser.add_argument(
        "audio", type=str,
        help="输入音频文件路径 (支持 mp3, wav, flac, ogg, m4a 等)",
    )
    parser.add_argument(
        "-o", "--output", type=str, default="./output",
        help="输出目录 (默认: ./output)",
    )
    parser.add_argument(
        "-m", "--model", type=str, default="htdemucs",
        choices=["htdemucs", "htdemucs_ft", "hdemucs_mmi", "htdemucs_6s"],
        help="Demucs 源分离模型 (默认: htdemucs)",
    )
    parser.add_argument(
        "--onset-threshold", type=float, default=0.5,
        help="basic-pitch 音符起始阈值 (0-1, 越低越多音符, 默认: 0.5)",
    )
    parser.add_argument(
        "--frame-threshold", type=float, default=0.3,
        help="basic-pitch 帧激活阈值 (0-1, 默认: 0.3)",
    )
    parser.add_argument(
        "--min-note-length", type=float, default=58.0,
        help="最小音符长度 (毫秒, 默认: 58)",
    )
    parser.add_argument(
        "--checkpoint", type=str, default=None,
        help="piano_transcription 模型检查点路径 (默认自动查找)",
    )
    parser.add_argument(
        "--midi", action="store_true",
        help="开启转录和制谱 (默认仅分离, 不转录不制谱)",
    )
    parser.add_argument(
        "--solo", type=str, default=None, metavar="STEM",
        help="仅提取指定声部, 其余合并为 other.wav "
             "(如 --solo vocals, --solo piano)",
    )
    parser.add_argument(
        "--silence-threshold", type=float, default=0.001, metavar="RMS",
        help="静音检测阈值 RMS (默认: 0.001 ≈ -60dBFS, 低于此值跳过)",
    )
    parser.add_argument(
        "--no-pdf", action="store_true",
        help="跳过 PDF 生成, 只输出 MIDI 和 MusicXML",
    )
    parser.add_argument(
        "--skip-separation", action="store_true",
        help="跳过分音频离, 使用已有 stems/ 目录",
    )
    parser.add_argument(
        "--skip-transcribe", action="store_true",
        help="跳过 MIDI 转写, 使用已有 midi/ 目录",
    )

    args = parser.parse_args()

    audio_path = Path(args.audio)
    if not audio_path.exists():
        print(f"错误: 找不到文件 '{args.audio}'", file=sys.stderr)
        sys.exit(1)

    print(f"输入文件: {audio_path}")
    print(f"输出目录: {args.output}")
    print(f"分离模型: {args.model}")
    print(f"生成 PDF: {'否' if args.no_pdf else '是'}")
    print(f"跳过分离: {'是' if args.skip_separation else '否'}")
    print(f"跳过转写: {'是' if args.skip_transcribe else '否'}")
    print(f"保留 MIDI: {'是' if args.midi else '否 (仅分离音轨)'}")
    print()

    # First-run notice: check which models need downloading
    _ckpt = Path(args.checkpoint) if args.checkpoint else (
        Path(__file__).parent / "note_F1=0.9677_pedal_F1=0.9186.pth")
    _lily = (Path(__file__).parent / "lilypond" / "bin" / "lilypond")
    print("—" * 50)
    print("首次运行提示：")
    print(f"  • Demucs 模型: 首次加载时将自动下载 (~80 MB)")
    if _ckpt.exists():
        print(f"  • 转录检查点: ✓ 已就绪")
    elif not args.skip_transcribe and args.midi:
        print(f"  • 转录检查点: 未找到，将自动下载 (~165 MB)")
    if _lily.exists():
        print(f"  • LilyPond: ✓ 已就绪")
    elif not args.no_pdf and args.midi:
        print(f"  • LilyPond: 未找到，PDF 将跳过（仅生成 MusicXML）")
    print("—" * 50)
    print()

    result = run_pipeline(
        audio_path=audio_path,
        output_dir=Path(args.output),
        demucs_model=args.model,
        onset_threshold=args.onset_threshold,
        frame_threshold=args.frame_threshold,
        minimum_note_length=args.min_note_length,
        skip_pdf=args.no_pdf,
        skip_separation=args.skip_separation,
        skip_transcribe=args.skip_transcribe,
        checkpoint_path=args.checkpoint,
        output_midi=args.midi,
        silence_threshold=args.silence_threshold,
        solo_stem=args.solo,
    )

    # ── Print summary ──
    print("\n" + "=" * 60)
    print(" ✅ 处理完成!")
    print("=" * 60)
    print(f"\n分离音轨 ({len(result['stems'])} 个):")
    for p in result["stems"]:
        print(f"  • {p}")

    if result["midi"]:
        print(f"\nMIDI 文件 ({len(result['midi'])} 个):")
        for p in result["midi"]:
            print(f"  • {p}")
    else:
        print(f"\nMIDI 文件: 未生成 (使用 --midi 开启)")

    if result["sheets"]:
        print(f"\n乐谱文件:")
        for s in result["sheets"]:
            pdf_status = "✓" if s["pdf"] else "✗ (未生成)"
            print(f"  • {s['name']}:  MusicXML={s['musicxml'].name}  PDF={pdf_status}")
    elif not result["midi"]:
        print(f"\n使用 --midi 可开启转录和制谱")

    print()


if __name__ == "__main__":
    main()
