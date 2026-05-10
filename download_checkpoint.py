#!/usr/bin/env python3
"""Download piano_transcription_inference checkpoint."""
import os
import sys
import requests

# Try multiple URLs in order of preference
URLS = [
    "https://zenodo.org/record/4034264/files/CRNN_note_F1%3D0.9677_pedal_F1%3D0.9186.pth?download=1",
]

# Write to workspace directory first, then user can move
DST = os.path.join(os.path.dirname(os.path.abspath(__file__)), 
                   "note_F1=0.9677_pedal_F1=0.9186.pth")

print(f"Target: {DST}")

for url in URLS:
    print(f"Trying: {url[:80]}...")
    try:
        r = requests.get(url, stream=True, timeout=60)
        r.raise_for_status()
        total = int(r.headers.get("content-length", 0))
        downloaded = 0
        with open(DST, "wb") as f:
            for chunk in r.iter_content(chunk_size=65536):
                f.write(chunk)
                downloaded += len(chunk)
                if total:
                    print(f"\r  {downloaded}/{total} ({downloaded*100//total}%)  ", 
                          end="", flush=True)
        print(f"\nDone: {DST} ({downloaded} bytes)")
        sys.exit(0)
    except Exception as e:
        print(f"  Failed: {e}")
        continue

print("All URLs failed.")
sys.exit(1)
