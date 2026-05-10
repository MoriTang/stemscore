#!/bin/bash
# Build standalone executable for StemScore pipeline
# Output: dist/stemscore/stemscore  (~1.2 GB onedir, includes checkpoint)
#
# Usage:
#   ./build.sh          # Incremental build (keeps Analysis cache, ~30s)
#   ./build.sh --clean  # Full rebuild (~3-4 min)
#
# Requires: venv with all dependencies installed

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

CLEAN=false
if [[ "${1:-}" == "--clean" ]]; then
    CLEAN=true
fi

# Ensure checkpoint exists
if [ ! -f "note_F1=0.9677_pedal_F1=0.9186.pth" ]; then
    echo "Downloading checkpoint..."
    source venv/bin/activate
    python3 download_checkpoint.py
fi

# Build with PyInstaller
source venv/bin/activate

if $CLEAN; then
    echo "Clean build (full)..."
    rm -rf build dist
else
    echo "Incremental build (keeping Analysis cache)..."
    rm -rf dist
fi

mkdir -p /tmp/pyinstaller_config
PYINSTALLER_CONFIG_DIR=/tmp/pyinstaller_config \
    python3 -m PyInstaller stemscore.spec --noconfirm

echo ""
echo "✓ Build complete: dist/stemscore/stemscore"
echo "  Size: $(du -sh dist/stemscore | cut -f1)"
echo ""
echo "Usage:"
echo "  dist/stemscore/stemscore audio.mp3"
echo "  dist/stemscore/stemscore audio.mp3 --midi"
echo "  dist/stemscore/stemscore --help"
