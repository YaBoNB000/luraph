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

# VM examples: a representative subset (VM containers are ~300KB each,
# keep the repo light — full VM coverage lives in the test matrix)
for case in basics functions game_loop; do
	"$TOOL" --vm --dialect 5.1 --seed $SEED "$ROOT/tests/cases/$case.lua" "$OUT/$case.vm.5.1.lua"
done

# v15 structural-parity examples (Route A): full coverage over all corpus
# cases, Luau-only profile (module table + CPS bootstrap,
# docs/v15-structural-parity-plan.md). Containers are ~40-50KB each,
# so full coverage keeps the repo light.
for case in "$ROOT"/tests/cases/*.lua; do
	base="$(basename "$case" .lua)"
	"$TOOL" --preset v15 --dialect luau --seed $SEED "$case" "$OUT/$base.v15.luau.lua"
done

# 增量⑱ (输入绑定/激活门): bound artifact for the gate target.
# NOTE: the activation string used here is a DEMO value (documented in
# examples/README.md); a real deployment keeps it out of the repo.
# The bound artifact itself contains NO trace of the activation.
"$TOOL" --preset v15 --dialect luau --bind-key "luraph-2026" --seed $SEED \
	"$ROOT/tests/cases/bind_gate.lua" "$OUT/bind_gate.bound.v15.luau.lua"
echo "generated $(ls "$OUT" | wc -l) examples in $OUT"
