#!/usr/bin/env python3
"""
Download LilyPond for PDF sheet music generation.

LilyPond is optional — without it, only MusicXML files are generated.
This script downloads the official LilyPond macOS binary and extracts it
to the project's lilypond/ directory.

Usage:
    python3 download_lilypond.py          # Download and extract
    python3 download_lilypond.py --check  # Only check if installed
"""

import os
import sys
import shutil
import tarfile
import urllib.request
from pathlib import Path

LILYPOND_VERSION = "2.26.0"
LILYPOND_URL = (
    f"https://gitlab.com/lilypond/lilypond/-/releases/v{LILYPOND_VERSION}/downloads/"
    f"lilypond-{LILYPOND_VERSION}-darwin-x86_64.tar.gz"
)
TARGET_DIR = Path(__file__).resolve().parent / "lilypond"


def is_installed() -> bool:
    """Check if LilyPond is already available."""
    lily_bin = TARGET_DIR / "bin" / "lilypond"
    return lily_bin.exists() and os.access(lily_bin, os.X_OK)


def _download_progress(block_num, block_size, total_size):
    """Progress reporthook for urlretrieve."""
    if total_size > 0:
        downloaded = block_num * block_size
        pct = min(int(downloaded * 100 / total_size), 100)
        print(f"\r  下载中... {pct}% ({downloaded//1048576}/{total_size//1048576} MB)",
              end="", flush=True)


def download():
    """Download and extract LilyPond."""
    if is_installed():
        print(f"✓ LilyPond {LILYPOND_VERSION} already installed at {TARGET_DIR}")
        return

    print(f"Downloading LilyPond {LILYPOND_VERSION}...")
    print(f"  URL: {LILYPOND_URL}")

    tar_path = TARGET_DIR.parent / f"lilypond-{LILYPOND_VERSION}.tar.gz"

    try:
        # Download with progress
        urllib.request.urlretrieve(LILYPOND_URL, tar_path, reporthook=_download_progress)
        print(f"\n  Downloaded to {tar_path}")

        # Extract
        print("  Extracting...")
        with tarfile.open(tar_path, "r:gz") as tar:
            # LilyPond tar extracts to lilypond-2.26.0/
            tar.extractall(path=TARGET_DIR.parent, filter="data")

        # Rename extracted dir to lilypond/
        extracted = TARGET_DIR.parent / f"lilypond-{LILYPOND_VERSION}"
        if extracted.exists() and not TARGET_DIR.exists():
            shutil.move(str(extracted), str(TARGET_DIR))

        # Clean up tar
        tar_path.unlink(missing_ok=True)

        print(f"✓ LilyPond {LILYPOND_VERSION} installed to {TARGET_DIR}")
        print(f"  Binary: {TARGET_DIR}/bin/lilypond")

    except Exception as e:
        print(f"✗ Download failed: {e}", file=sys.stderr)
        print("  You can install LilyPond manually:", file=sys.stderr)
        print("    brew install lilypond", file=sys.stderr)
        print("  Or skip PDF generation with --no-pdf", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    if "--check" in sys.argv:
        if is_installed():
            print("OK")
        else:
            print("NOT INSTALLED")
            sys.exit(1)
    else:
        download()
