#!/bin/bash
# M6: every named preset × every corpus case × applicable dialect.
# Compares stdout+exit against the original. A timeout is a FAIL.
# vm/max use the 120s VM timeout; low/medium/high use 30s.
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TOOL="$ROOT/target/release/luraph-rs"
TOOLS_BIN=/home/user/tools/bin
if [ ! -x "$TOOLS_BIN/lua51" ] || [ ! -x "$TOOLS_BIN/luau" ]; then
	TOOLS_BIN="$(cd "$(dirname "$0")/../../.tools/bin" && pwd)"
fi
LUA51="$TOOLS_BIN/lua51"
LUAU="$TOOLS_BIN/luau"
LUAUC="$TOOLS_BIN/luau-compile"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

PRESETS="${1:-low medium high vm max}"
pass=0
fail=0
failed=()

for preset in $PRESETS; do
	case $preset in
		vm|max) to=120 ;;
		*) to=30 ;;
	esac
	for case in "$ROOT"/tests/cases/*.lua; do
		base="$(basename "$case" .lua)"
		if [[ "$base" == luau_* ]]; then
			dialects="luau"
		else
			dialects="5.1 luau"
		fi
		for d in $dialects; do
			out="$TMP/${preset}__${base}__${d}.lua"
			if ! timeout 90 "$TOOL" --preset "$preset" --dialect "$d" --seed 42 "$case" "$out" 2>"$TMP/err.txt"; then
				echo "FAIL [tool] $preset $base ($d): $(head -1 "$TMP/err.txt")"
				fail=$((fail + 1)); failed+=("$preset/$base/$d/tool")
				continue
			fi
			if [[ "$d" == "5.1" ]]; then interp=$LUA51; else interp=$LUAU; fi
			o1="$(timeout $to "$interp" "$case" 2>&1)"; c1=$?
			o2="$(timeout $to "$interp" "$out" 2>&1)"; c2=$?
			if [[ "$c1" != "$c2" || "$o1" != "$o2" ]]; then
				echo "FAIL [run]  $preset $base ($d): exit $c1 vs $c2"
				fail=$((fail + 1)); failed+=("$preset/$base/$d/run")
			else
				pass=$((pass + 1))
			fi
			if [[ "$d" == "luau" ]]; then
				if ! timeout 60 "$LUAUC" "$out" >/dev/null 2>"$TMP/luac_err.txt"; then
					echo "FAIL [luac] $preset $base: $(head -1 "$TMP/luac_err.txt")"
					fail=$((fail + 1)); failed+=("$preset/$base/luac")
				else
					pass=$((pass + 1))
				fi
			fi
		done
	done
done

echo "=================================================="
echo "PRESET PASS: $pass   FAIL: $fail"
if [[ $fail -gt 0 ]]; then
	for f in "${failed[@]}"; do echo "  failed: $f"; done
	exit 1
fi
echo "ALL PRESETS GREEN"
