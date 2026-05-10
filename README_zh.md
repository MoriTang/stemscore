# StemScore

从音频文件自动分离乐器、转录 MIDI、生成分谱。

```
一首歌 → 4~6 轨 WAV → MIDI → MusicXML + PDF 乐谱
```

## 快速开始

```bash
git clone <repo>
cd transcription
python3 -m venv venv
source venv/bin/activate
pip install -r requirements.txt

# 下载模型检查点（~165 MB，首次必需）
python3 download_checkpoint.py

# 仅分离音轨（默认）
python3 main.py song.mp3

# 分离 + 转录 + 乐谱
python3 main.py song.mp3 --midi
```

## 首次运行：下载说明

首次运行较慢，因为需要**一次性**下载并缓存以下依赖：

| 下载内容 | 大小 | 触发时机 | 说明 |
|----------|------|----------|------|
| `pip install -r requirements.txt` | ~2 GB | 环境搭建 | 主要是 PyTorch（~1.5 GB）；每个虚拟环境仅需一次 |
| 模型检查点 | ~165 MB | 首次执行 `python3 download_checkpoint.py` | piano-transcription 权重，存放在项目根目录 |
| LilyPond | ~15 MB | 首次执行 `python3 download_lilypond.py` | 仅 PDF 输出需要；可选 |
| Demucs 模型 | ~80 MB | 首次实际运行时 | 自动从 torch hub 下载至 `~/.cache/torch/` |
| basic-pitch 模型 | ~30 MB | 首次转录时 | 首次 `--midi` 运行时自动下载 |

**实际情况**：全新环境（新建 venv，无缓存）首次启动需 5–15 分钟，取决于网络速度。之后所有文件已缓存，后续运行只需几秒到几分钟（取决于音频长度）。

可以提前运行模型检查点和 LilyPond 下载脚本：
```bash
python3 download_checkpoint.py   # 预下载模型检查点
python3 download_lilypond.py     # 预下载 LilyPond（可选）
```

## 使用方式

```bash
python3 main.py <音频文件> [选项]
```

| 选项 | 说明 |
|------|------|
| `-o DIR` | 输出目录（默认 `./output`） |
| `-m MODEL` | 分离模型（默认 `htdemucs`） |
| `--midi` | 开启转录和制谱 |
| `--no-pdf` | 跳过 PDF，只输出 MusicXML |
| `--fast` | 快速模式，分离约 2x 加速 |
| `--solo STEM` | 仅提取指定声部，其余合并 |
| `--skip-separation` | 跳过分离，使用已有 stems/ |
| `--skip-transcribe` | 跳过转录，使用已有 midi/ |
| `--silence-threshold RMS` | 静音检测阈值（默认 0.001） |
| `-h` | 查看完整参数 |

### 示例

```bash
# 最基本：只分离 4 轨 WAV
python3 main.py song.mp3

# 完整流程：分离 + MIDI + 乐谱
python3 main.py song.mp3 --midi

# 快速模式
python3 main.py song.mp3 --midi --fast

# 卡拉OK：提取人声，其余合并为伴奏
python3 main.py song.mp3 --solo vocals --midi

# 6 轨分离（实验性，guitar 尚可、piano 有杂音）
python3 main.py song.mp3 -m htdemucs_6s --midi

# 跳过分离和转录，只重新生成乐谱
python3 main.py song.mp3 --skip-separation --skip-transcribe --midi
```

## 输出结构

```
output/
├── stems/          # 分离后的 WAV 音轨
│   ├── bass.wav
│   ├── drums.wav
│   ├── other.wav
│   └── vocals.wav
├── midi/           # MIDI 文件（需 --midi）
├── musicxml/       # MusicXML 乐谱（需 --midi）
└── pdf/            # PDF 乐谱（需 --midi + LilyPond）
```

## 模型选择

| 模型 | 声轨数 | 说明 |
|------|--------|------|
| `htdemucs` | 4 | 默认：drums, bass, other, vocals |
| `htdemucs_ft` | 4 | 微调版，相同声轨 |
| `hdemucs_mmi` | 4 | 多乐器训练，相同声轨 |
| `htdemucs_6s` | 6 | 实验性：+ guitar, piano |

## 乐器乐谱优化

制谱时根据声部自动应用：

| 声部 | 谱号 | 格式 |
|------|------|------|
| bass | 低音谱号 | 单行 |
| drums | 打击乐谱号 | 节奏记谱 |
| guitar | 低八度高音谱号 | 单行 |
| piano | 大谱表 | 高低音双行 |
| vocals | 高音谱号 | 单行 |

## 可选：PDF 生成

PDF 需要 LilyPond。不装也能用——只影响 PDF，MusicXML 正常产出。

```bash
# 下载 LilyPond（~15 MB，一次性）
python3 download_lilypond.py

# 或用 brew（macOS）
brew install lilypond
```

MusicXML 可导入 [MuseScore](https://musescore.org)（免费）直接查看编辑。

## 构建独立可执行文件

```bash
./build.sh
# 产物：dist/transcribe/transcribe
# 使用：dist/transcribe/transcribe song.mp3 --midi
```

## 古典音乐

可以用，但分离精度会下降——Demucs 训练数据以流行/摇滚为主。管弦乐大部分乐器会落入 `other` 轨，无法拆分为独立分谱。静音检测会自动跳过空轨。

## 许可证

MIT
