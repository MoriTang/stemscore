#!/bin/bash
# Quick help for transcribe — no Python startup overhead
cat << 'HELP'
transcribe — 从音频文件自动生成多乐器分谱

用法:
  transcribe <音频文件> [选项]

选项:
  -o, --output DIR        输出目录 (默认: ./output)
  -m, --model MODEL       Demucs 模型: htdemucs (默认), htdemucs_ft, hdemucs_mmi
  --midi                  开启转录和制谱 (默认仅分离音轨)
  --no-pdf                跳过 PDF 生成 (仍输出 MusicXML)
  --fast                  快速模式: 分离约 2x 加速 (质量略降)
  --solo STEM             仅提取指定声部, 其余合并为 other.wav
                          可选: drums, bass, other, vocals
  --skip-separation       跳过分离, 使用已有 stems/ 目录
  --silence-threshold RMS 静音检测阈值 (默认: 0.001 ≈ -60dBFS)
  --onset-threshold N     音符起始灵敏度 0-1 (默认: 0.5)
  --frame-threshold N     帧激活阈值 0-1 (默认: 0.3)
  --min-note-length MS    最小音符长度毫秒 (默认: 58)
  --checkpoint PATH       piano_transcription 检查点路径 (默认自动查找)
  -h, --help              显示此帮助

示例:
  transcribe song.mp3                          # 仅分离 4 轨 WAV
  transcribe song.mp3 --midi                   # 分离 + MIDI + 乐谱
  transcribe song.mp3 --solo vocals --midi     # 提取人声 + 伴奏, 制谱
  transcribe song.mp3 --fast --midi            # 快速模式

输出目录结构:
  output/
  ├── stems/       WAV 音轨 (drums, bass, other, vocals)
  ├── midi/        MIDI 文件 (需 --midi)
  ├── musicxml/    MusicXML 乐谱 (需 --midi)
  └── pdf/         PDF 乐谱 (需 --midi, 且安装 Lilypond)

模型说明:
  htdemucs      - 4 声源: drums, bass, other, vocals
  htdemucs_ft   - 微调版, 相同 4 声源
  hdemucs_mmi   - 多乐器训练, 相同 4 声源, 权重不同

注意事项:
  - 首次运行会下载 Demucs 模型 (~350MB)
  - PDF 需要安装 Lilypond (https://lilypond.org) 或 MuseScore
  - 古典管弦乐分离精度受限于训练数据, 主要集中在流行/摇滚乐器
HELP
