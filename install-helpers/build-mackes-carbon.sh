#!/usr/bin/env bash
# build-mackes-carbon.sh — transform IBM Carbon SVGs into a freedesktop
# GTK icon theme at data/icons/Mackes-Carbon/.
#
# Carbon source: github.com/carbon-design-system/carbon, Apache 2.0.
# Default source location is /tmp/carbon-icons/package/svg/32 (where
# install-helpers/fetch-carbon-icons.sh drops the clone). Override via
# CARBON_SVG_DIR.
#
# Idempotent — safe to re-run. Each SVG gets fill="currentColor" injected
# on the root <svg> so GTK and the Mackes panel CSS can recolor.

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
SRC="${CARBON_SVG_DIR:-/tmp/carbon-icons/package/svg/32}"
OUT="$ROOT/data/icons/Mackes-Carbon"
MAP="$HERE/mackes-carbon.map"

[ -d "$SRC" ] || { echo "ERROR: Carbon source not found at $SRC" >&2; exit 1; }
[ -f "$MAP" ] || { echo "ERROR: name map not found at $MAP" >&2; exit 1; }

# Fresh tree so deleted mappings actually disappear.
rm -rf "$OUT"
mkdir -p "$OUT/scalable"/{actions,apps,categories,devices,emblems,mimetypes,places,status}

# Inject fill="currentColor" on the <svg> root. Skip if a fill is already set.
recolor() {
    sed -E '/<svg[^>]*\bfill=/!s|<svg([^>]*)>|<svg\1 fill="currentColor">|'
}

mapped=0
missed=0
while read -r fdname category carbon; do
    [[ -z "$fdname" || "$fdname" == \#* ]] && continue
    src="$SRC/$carbon.svg"
    if [ ! -f "$src" ]; then
        echo "  miss: $fdname → $carbon" >&2
        missed=$((missed + 1))
        continue
    fi
    out_sym="$OUT/scalable/$category/${fdname}.svg"
    out_plain="$OUT/scalable/$category/${fdname%-symbolic}.svg"
    recolor < "$src" > "$out_sym"
    cp "$out_sym" "$out_plain"
    mapped=$((mapped + 2))
done < "$MAP"

shopt -s nullglob
dumped=0
for svg in "$SRC"/*.svg; do
    out="$OUT/scalable/apps/$(basename "$svg")"
    [ -f "$out" ] && continue
    recolor < "$svg" > "$out"
    dumped=$((dumped + 1))
done

cat > "$OUT/index.theme" <<'EOF'
[Icon Theme]
Name=Mackes-Carbon
Comment=Mackes Shell — IBM Carbon Design System icons (Apache 2.0)
Inherits=hicolor,Adwaita
Directories=scalable/actions,scalable/apps,scalable/categories,scalable/devices,scalable/emblems,scalable/mimetypes,scalable/places,scalable/status
Example=view-grid-symbolic

[scalable/actions]
Context=Actions
Size=32
MinSize=8
MaxSize=512
Type=Scalable

[scalable/apps]
Context=Applications
Size=32
MinSize=8
MaxSize=512
Type=Scalable

[scalable/categories]
Context=Categories
Size=32
MinSize=8
MaxSize=512
Type=Scalable

[scalable/devices]
Context=Devices
Size=32
MinSize=8
MaxSize=512
Type=Scalable

[scalable/emblems]
Context=Emblems
Size=32
MinSize=8
MaxSize=512
Type=Scalable

[scalable/mimetypes]
Context=MimeTypes
Size=32
MinSize=8
MaxSize=512
Type=Scalable

[scalable/places]
Context=Places
Size=32
MinSize=8
MaxSize=512
Type=Scalable

[scalable/status]
Context=Status
Size=32
MinSize=8
MaxSize=512
Type=Scalable
EOF

if [ -f "$SRC/../../LICENSE" ]; then
    cp "$SRC/../../LICENSE" "$OUT/LICENSE"
elif [ -f "/tmp/carbon-icons/package/LICENSE" ]; then
    cp /tmp/carbon-icons/package/LICENSE "$OUT/LICENSE"
fi

cat > "$OUT/NOTICE" <<'EOF'
Mackes-Carbon is derived from the IBM Carbon Design System icon set.

Upstream: https://github.com/carbon-design-system/carbon/tree/main/packages/icons
License:  Apache License 2.0 (see LICENSE)

The original SVGs are unmodified geometry; the only transformation is
the injection of fill="currentColor" on the root <svg> element so the
icons recolor with the application's GTK style context. Files are
renamed and reorganized into a freedesktop icon-theme directory layout.
EOF

echo "Mackes-Carbon built at $OUT"
echo "  Curated freedesktop mappings: $mapped files ($missed misses)"
echo "  Carbon apps/ dump:            $dumped files"
