# StemFlow

本地优先的 AI 音乐分轨工具，支持音频文件分轨，也可以录制并实时分离 macOS 正在播放的立体声音频。

> 项目原名 StemScore。GitHub 仓库和产品展示名已更新为 StemFlow；为避免重新下载模型、丢失历史输出访问权限，本地数据目录仍保持不变。

## 主要功能

- **文件分轨**：导入 WAV、MP3、FLAC、M4A/AAC 或 OGG，生成鼓、贝斯、其他和人声四轨 WAV。
- **实时分轨**：录制 macOS 默认输出设备的立体声混音，边录制边执行 ONNX 推理。
- **实时可视化**：显示四轨滚动波形、输入电平和随鼓轨强度变化的呼吸灯。
- **分轨试听**：文件处理或录制完成后，可分别播放、暂停和定位每条音轨。
- **本地处理**：音频不上传，Rust 音频链路不依赖 Python 或 PyTorch。

## 桌面版快速开始

```bash
git clone https://github.com/MoriTang/stemflow.git
cd stemflow/desktop
npm install
npm run dev
```

macOS 14.6 或更高版本可直接捕获系统输出，不需要安装虚拟声卡。开发环境还需要 Rust 1.88+、Node.js、npm 和 Xcode Command Line Tools。

首次启用分轨时，应用会下载并校验一个约 **106 MB** 的 HS-TasNet ONNX 模型；后续文件分轨和实时分轨共用缓存，不会重复下载。

```bash
# 构建 macOS 应用
npm run build -- --bundles app
```

桌面版的详细运行方式、输出结构和当前限制见 [desktop/README.md](desktop/README.md)。

## 数据与输出

| 内容 | 位置 |
|------|------|
| ONNX 模型缓存 | `~/Library/Application Support/com.MoriTang.StemScore/models/` |
| 实时录制 | `~/Music/StemScore Recordings/` |
| 文件分轨 | `~/Music/StemScore Separations/` |

上述目录保留原名是为了兼容已下载的模型和现有输出。

## Python 命令行版（兼容保留）

仓库仍保留 Demucs 命令行流程，适合对延迟不敏感、更看重离线分轨质量的场景。可选的 `--midi` 还会生成 MIDI 和 MusicXML；不再生成 PDF 乐谱。

```bash
cd stemflow
python3 -m venv venv
source venv/bin/activate
pip install -r requirements.txt
python3 main.py song.mp3
```

## Python CLI 使用方式

```bash
python3 main.py <音频文件> [选项]
```

| 选项 | 说明 |
|------|------|
| `-o DIR` | 输出目录（默认 `./output`） |
| `-m MODEL` | 分离模型（默认 `htdemucs`） |
| `--midi` | 开启转录和制谱 |
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
└── musicxml/       # MusicXML 乐谱（需 --midi）
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

MusicXML 可导入 [MuseScore](https://musescore.org)（免费）直接查看编辑。

## 构建独立可执行文件

```bash
./build.sh
# 产物：dist/stemscore/stemscore
# 使用：dist/stemscore/stemscore song.mp3 --midi
```

## 古典音乐

可以用，但分离精度会下降——Demucs 训练数据以流行/摇滚为主。管弦乐大部分乐器会落入 `other` 轨，无法拆分为独立分谱。静音检测会自动跳过空轨。

## 许可证

MIT
