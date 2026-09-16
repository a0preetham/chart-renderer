#!/usr/bin/env bash
# Measures the real shipped wasm bundle size per text-backend feature.
#
# The handoff carried estimates for these; this replaces them with measurements.
# It reports the wasm-bindgen + wasm-opt output — what actually ships — both raw
# and gzipped, since Cloudflare applies its script-size limit to the compressed
# upload.
set -euo pipefail

cd "$(dirname "$0")/crates/wasm"

printf '%-14s %14s %14s\n' feature shipped gzipped
printf '%-14s %14s %14s\n' ------- ------- -------

for feature in simple-text full-text; do
  out="pkg-$feature"
  if ! wasm-pack build --release --target web --out-dir "$out" -- \
       --no-default-features --features "$feature" >/dev/null 2>&1; then
    printf '%-14s %14s\n' "$feature" "BUILD FAILED"
    continue
  fi
  wasm="$out/chart_renderer_wasm_bg.wasm"
  printf '%-14s %14s %14s\n' \
    "$feature" "$(stat -c%s "$wasm")" "$(gzip -9 -c "$wasm" | wc -c)"
done

cat <<'EOF'

Workers script limits: 3MB (Free), 10MB (Paid), against the compressed upload.

CAVEAT on full-text: CosmicShaper is still a stub, so nothing calls into
cosmic-text and LTO strips it from the binary entirely. That number therefore
measures "no text backend at all", not "full text", and is smaller than
simple-text only because it drops ab_glyph and the embedded font. It has to be
re-measured once the backend is implemented — until then the handoff's
1-1.5MB gzipped estimate for full-text stands unverified.
EOF
