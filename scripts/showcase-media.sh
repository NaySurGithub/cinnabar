#!/usr/bin/env bash
# Prepares the localserver showcase intro (developer media path) under .local/showcase, which git
# ignores: fetches the CC-BY 3.0 Sintel trailer (© Blender Foundation, durian.blender.org),
# transcodes it to the client's WebM AV1+Opus profile, signs the `showcase` client part bundle,
# seeds it into a client's cache and prints the server and client commands.
# Usage: scripts/showcase-media.sh <client-user-data-dir> <world-dir> [media-addr]
set -euo pipefail

if [ "$#" -lt 2 ] || [ "$#" -gt 3 ]; then
	echo "usage: $0 <client-user-data-dir> <world-dir> [media-addr]" >&2
	exit 2
fi
user_data=$1
world=$2
addr=${3:-127.0.0.1:19443}

source_url=https://download.blender.org/durian/trailer/sintel_trailer-720p.mp4
source_sha256=cb0fe73fc0a7d543459996c0cdab730997e6eac1013d3ede18796f777cb7f273

root=$(git -C "$(dirname "$0")" rev-parse --show-toplevel)
out=$root/.local/showcase
target=${CARGO_TARGET_DIR:-$root/target}
mkdir -p "$out/media" "$out/assets/media" "$out/cxb" "$world"

for tool in ffmpeg curl shasum cargo; do
	command -v "$tool" >/dev/null || {
		echo "$tool is required" >&2
		exit 1
	}
done

source=$out/sintel_trailer-720p.mp4
if [ ! -f "$source" ]; then
	curl -sSfL -o "$source.part" "$source_url"
	mv "$source.part" "$source"
fi
echo "$source_sha256  $source" | shasum -a 256 -c - >/dev/null

# Profile: progressive 8-bit 4:2:0 limited-range BT.709 AV1 up to 1280x720 at 24 fps, Opus 48 kHz
# stereo in 20 ms packets, no chapters or metadata.
raw=$out/intro.raw.webm
ffmpeg -hide_banner -loglevel error -y -i "$source" -map 0:v:0 -map 0:a:0 \
	-vf "scale=1280:720,fps=24,format=yuv420p,setparams=range=tv:color_primaries=bt709:color_trc=bt709:colorspace=bt709" \
	-c:v libsvtav1 -preset 8 -crf 36 -g 48 \
	-c:a libopus -ar 48000 -ac 2 -b:a 128k -frame_duration 20 \
	-map_metadata -1 -map_chapters -1 -fflags +bitexact -write_crc32 0 -f webm "$raw"
ffmpeg -hide_banner -loglevel error -y -ss 20 -i "$source" -frames:v 1 -vf scale=640:-2 \
	"$out/assets/media/intro.png"

cxb() {
	cargo run --locked -q -p cinnabar-cxb -- "$@"
}
for seed in publisher server; do
	[ -f "$out/$seed.seed" ] || cxb keygen "$out/$seed.seed" >/dev/null
done

# The descriptor URL is a loopback origin, which the client accepts only in its developer media mode.
ca=$world/extension-media-ca.pem
CINNABAR_DEV_SERVER_EXPERIENCES=1 CINNABAR_DEV_MEDIA_CA=$ca cxb media \
	--webm "$raw" --url "https://$addr/intro.webm" --id showcase.intro --poster media/intro.png \
	--out-webm "$out/media/intro.webm" --out-descriptor "$out/assets/media/intro.json"

cargo build --locked -q --release --target wasm32-unknown-unknown -p showcase-screen
cxb build --manifest "$root/examples/client-parts/showcase-screen/manifest.toml" \
	--component "$target/wasm32-unknown-unknown/release/showcase_screen.wasm" \
	--publisher-seed "$out/publisher.seed" --out "$out/cxb/showcase.cxb" --assets "$out/assets"
cxb seed-cache --cxb "$out/cxb/showcase.cxb" --user-data "$user_data"

cat <<EOF

Prepared $out. Video: "Sintel" trailer, (c) Blender Foundation, CC BY 3.0.

Server (from $root/tools/localserver):
  GOWORK=off go run . -dir "$world" -addr 127.0.0.1:19132 \\
    -extension-key "$out/server.seed" -extension-audience 127.0.0.1:19132 \\
    -extension-cxb "$out/cxb" -extension-media "$out/media" -extension-media-addr $addr

Client (join 127.0.0.1:19132, allow the client part, then run /intro):
  cargo build -p mod-host --features media
  CINNABAR_DEV_SERVER_EXPERIENCES=1 CINNABAR_DEV_MEDIA_CA="$ca" cargo run -p bedrock-client
EOF
