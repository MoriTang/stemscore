#!/bin/bash
# Build standalone executable for transcription pipeline
# Output: dist/stemscore/stemscore  (~1 GB onedir, includes checkpoint)
#
# Requires: venv with all dependencies installed

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# Ensure checkpoint exists (needed for build — bundled into executable)
if [ ! -f "note_F1=0.9677_pedal_F1=0.9186.pth" ]; then
    echo "Downloading checkpoint..."
    source venv/bin/activate
    python3 download_checkpoint.py
fi

# Build with PyInstaller
# PYINSTALLER_CONFIG_DIR redirects cache to /tmp to avoid macOS sandbox issues
source venv/bin/activate
rm -rf build dist
mkdir -p /tmp/pyinstaller_config

echo "Building executable..."
PYINSTALLER_CONFIG_DIR=/tmp/pyinstaller_config \
    pyinstaller stemscore.spec --noconfirm

echo ""
echo "✓ Build complete: dist/stemscore/stemscore"
echo "  Size: $(du -sh dist/stemscore | cut -f1)"
echo ""
echo "Usage:"
echo "  dist/stemscore/stemscore audio.mp3"
echo "  dist/stemscore/stemscore audio.mp3 --midi"
echo "  dist/stemscore/stemscore --help"
