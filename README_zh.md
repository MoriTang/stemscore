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

## Rust CLI

默认 CLI 与桌面端共用 Rust 解码、重采样、模型缓存和 ONNX 分轨核心，不安装也不加载 Python/PyTorch。

```bash
cargo build --release --manifest-path cli/Cargo.toml
./cli/target/release/stemflow separate song.mp3

# 或安装到 ~/.cargo/bin，之后可直接使用 stemflow 命令
cargo install --path cli
```

首次快速分轨会自动下载并校验共享模型，后续直接复用缓存。

```bash
# 指定输出目录
stemflow separate song.mp3 --output ./result

# 保留四条音轨，额外生成去人声伴奏
stemflow separate song.mp3 --solo vocals

# 查看、下载或校验共享模型
stemflow model status
stemflow model download
stemflow model verify
```

快速分轨输出：

```text
output/stems/
├── drums.wav
├── bass.wav
├── other.wav
└── vocals.wav
```

`--solo vocals` 会保留上述四个文件，并额外生成 `accompaniment-without-vocals.wav`，不会删除分轨结果。

## 可选高质量 Demucs 后端

只有显式选择高质量模式时才需要 Demucs。建议安装到独立虚拟环境：

```bash
python3 -m venv .venv-demucs
.venv-demucs/bin/pip install -r backends/demucs/requirements.txt

stemflow separate song.mp3 \
  --quality high \
  --python .venv-demucs/bin/python \
  --model htdemucs
```

也可设置 `STEMFLOW_DEMUCS_PYTHON` 避免每次传入 `--python`。默认 Rust 构建不携带该环境、Demucs 权重或 Python 解释器。项目已移除 PyInstaller、MIDI 转录和 MusicXML 生成功能。

## 当前限制

- 快速 HS-TasNet 模型固定为 44.1 kHz 立体声四轨，优先保证延迟，离线质量低于 Demucs。
- 高质量模式仍需外部 Python/PyTorch 环境。
- 桌面端系统音频捕获目前需要 macOS 14.6+；Rust 文件分轨可跨平台。
- 古典与管弦乐的大部分乐器会进入 `other` 轨。
- 请勿用 StemFlow 绕过 DRM 或处理无权录制的音频。

## 许可证

MIT
