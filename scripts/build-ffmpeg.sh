#!/bin/sh
# Minimal non-GPL FFmpeg from pinned upstream source; no Homebrew required.
set -eu
cd "$(dirname "$0")/.."
if [ "$(uname -s)" != Darwin ] || [ "$(uname -m)" != arm64 ]; then echo 'Requires Apple Silicon macOS and Xcode Command Line Tools.' >&2; exit 1; fi
command -v clang >/dev/null
mkdir -p artifacts/ffmpeg-source artifacts/ffmpeg
curl --fail --location --max-time 120 --output artifacts/ffmpeg-source/ffmpeg-9.0.2.tar.xz https://ffmpeg.org/releases/ffmpeg-9.0.2.tar.xz
printf '%s\n' '8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e  artifacts/ffmpeg-source/ffmpeg-9.0.2.tar.xz' | shasum -a 256 -c -
tar -xf artifacts/ffmpeg-source/ffmpeg-9.0.2.tar.xz -C artifacts/ffmpeg-source
cd artifacts/ffmpeg-source/ffmpeg-9.0.2
./configure --prefix="$(cd ../../ffmpeg && pwd)" --disable-everything --disable-autodetect --disable-doc --disable-debug --enable-ffmpeg --enable-videotoolbox --enable-encoder=h264_videotoolbox,rawvideo --enable-decoder=rawvideo,h264 --enable-parser=h264 --enable-demuxer=rawvideo,h264 --enable-muxer=h264,rawvideo --enable-protocol=pipe --enable-filter=crop,scale,format,null --enable-bsf=h264_metadata,dump_extradata
make -j "$(sysctl -n hw.logicalcpu)"
make install
cp COPYING.LGPLv2.1 ../../ffmpeg/
printf 'FFmpeg built from retained upstream source. Check artifacts/ffmpeg/bin/ffmpeg.\n'
