#!/bin/bash
# Regenerate the obfuscated examples for every test-corpus case.
# Output: luraph-rs/examples/<case>.5.1.lua  (shared cases, 5.1 target)
#         luraph-rs/examples/<case>.luau.lua (luau_* cases, luau target)
set -eu
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TOOL="$ROOT/target/release/luraph-rs"
OUT="$ROOT/examples"
SEED=42
mkdir -p "$OUT"
rm -f "$OUT"/*.lua

for case in "$ROOT"/tests/cases/*.lua; do
	base="$(basename "$case" .lua)"
	if [[ "$base" == luau_* ]]; then
		"$TOOL" --dialect luau --seed $SEED "$case" "$OUT/$base.luau.lua"
	else
		"$TOOL" --dialect 5.1 --seed $SEED "$case" "$OUT/$base.5.1.lua"
	fi
done
echo "generated $(ls "$OUT" | wc -l) examples in $OUT"
