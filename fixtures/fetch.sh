#!/usr/bin/env bash
set -euo pipefail
mkdir -p fixtures/raw
# CC0 entries are selected from raw.pixls.us/json/getrepository.php?set=all.
files=(
  "canon-cr3.CR3|https://raw.pixls.us/getfile.php/2663/nice/Canon%20-%20EOS%20M50%20-%20CRAW%20(3%3A2).CR3"
  "sony-arw.ARW|https://raw.pixls.us/getfile.php/782/nice/Sony%20-%20NEX-6%20-%2012bit%2012bit%20compressed%20(3%3A2).ARW"
  "nikon-nef.NEF|https://raw.pixls.us/getfile.php/750/nice/Nikon%20-%20D800%20-%2014bit%2014bit%20compressed%20(Lossless)%20(3%3A2).NEF"
  "fuji-raf.RAF|https://raw.pixls.us/getfile.php/745/nice/Fujifilm%20-%20X-E2S%20-%2014bit%2014bit%20uncompressed%20(3%3A2).RAF"
  "sample.dng|https://raw.pixls.us/getfile.php/752/nice/Leica%20-%20M9%20Digital%20Camera%20-%2016bit.DNG"
)
for item in "${files[@]}"; do
  name=${item%%|*}; url=${item#*|}
  [[ -e "fixtures/raw/$name" ]] && continue
  curl -fL --retry 2 -o "fixtures/raw/$name" "$url"
done
for f in fixtures/raw/*; do
  size=$(wc -c < "$f")
  (( size > 5242880 )) || { echo "$f is too small: $size bytes" >&2; exit 1; }
done
