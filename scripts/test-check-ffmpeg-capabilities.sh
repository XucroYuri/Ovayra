#!/usr/bin/env bash
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd); tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
cat > "$tmp/ffmpeg" <<'EOF'
#!/usr/bin/env bash
case "$2" in
-hwaccels) printf 'Hardware acceleration methods:\nvaapi\ncuda\nd3d11va\ndxva2\nvideotoolbox\n' ;;
-decoders) printf ' V..... vp9\n V..... h264_cuvid\n V..... hevc_cuvid\n' ;;
-encoders) printf ' V..... libvpx-vp9\n A..... libopus\n V..... h264_nvenc\n V..... hevc_nvenc\n V..... h264_mf\n V..... h264_videotoolbox\n V..... hevc_videotoolbox\n A..... aac_at\n' ;;
-filters) printf ' ... scale_vaapi\n' ;;
esac
EOF
chmod +x "$tmp/ffmpeg"
for target in macos-arm64-vt windows-x64-mf linux-x64-vaapi-wayland; do bash "$root/scripts/check-ffmpeg-capabilities.sh" --target-id "$target" --ffmpeg "$tmp/ffmpeg"; done
sed -i.bak 's/hevc_nvenc/hevc_missing/' "$tmp/ffmpeg"
if bash "$root/scripts/check-ffmpeg-capabilities.sh" --target-id windows-x64-mf --ffmpeg "$tmp/ffmpeg"; then exit 1; fi
