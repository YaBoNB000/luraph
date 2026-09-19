#!/bin/bash
# Regenerate the obfuscated examples for every test-corpus case.
# ㉖: 产品线 Luau-only——默认输出即 v15 管线（<case>.v15.luau.lua 就是
# 裸调用的产品形态）。旧 5.1 产物已从 examples 移除。
set -eu
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TOOL="$ROOT/target/release/luraph-rs"
OUT="$ROOT/examples"
SEED=42
mkdir -p "$OUT"
rm -f "$OUT"/*.lua





# v15 structural-parity examples (Route A): full coverage over all corpus
# cases, Luau-only profile (module table + CPS bootstrap,
# docs/v15-structural-parity-plan.md). Containers are ~40-50KB each,
# so full coverage keeps the repo light.
for case in "$ROOT"/tests/cases/*.lua; do
	base="$(basename "$case" .lua)"
	"$TOOL" --preset v15 --dialect luau --seed $SEED "$case" "$OUT/$base.v15.luau.lua"
done
# ㊴: 激活门 (--bind-key) 已退役——绑定态示例不再生成。
echo "generated $(ls "$OUT" | wc -l) examples in $OUT"
