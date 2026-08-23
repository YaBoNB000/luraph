#!/bin/bash
# M0 round-trip test matrix (mandatory workflow):
#   every corpus case x every applicable dialect x (original vs obfuscated
#   output) must produce identical stdout+exit code on the target
#   interpreter; luau targets additionally pass luau-compile; 5.1-target
#   output must also run under luau (5.1 code is valid Luau).
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TOOL="$ROOT/target/release/luraph-rs"
LUA51=/home/user/tools/bin/lua51
LUAU=/home/user/tools/bin/luau
LUAUC=/home/user/tools/bin/luau-compile
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

pass=0
fail=0
failed=()

check() { # name interp file
	local name="$1" interp="$2" file="$3"
	timeout 30 "$interp" "$file" 2>&1
}

for case in "$ROOT"/tests/cases/*.lua; do
	base="$(basename "$case")"
	if [[ "$base" == luau_* ]]; then
		dialects="luau"
	else
		dialects="5.1 luau"
	fi
	for d in $dialects; do
		out="$TMP/${base}__${d}.lua"
		if ! "$TOOL" --dialect "$d" "$case" "$out" 2>"$TMP/tool_err.txt"; then
			echo "FAIL [tool]  $base ($d): $(head -1 "$TMP/tool_err.txt")"
			fail=$((fail + 1)); failed+=("$base/$d/tool")
			continue
		fi
		if [[ "$d" == "5.1" ]]; then
			interp=$LUA51
		else
			interp=$LUAU
		fi
		o1="$(check "$base" "$interp" "$case")"; c1=$?
		o2="$(check "$base" "$interp" "$out")"; c2=$?
		if [[ "$c1" != "$c2" || "$o1" != "$o2" ]]; then
			echo "FAIL [run]   $base ($d): exit $c1 vs $c2"
			diff <(echo "$o1") <(echo "$o2") | head -6
			fail=$((fail + 1)); failed+=("$base/$d/run")
		else
			pass=$((pass + 1))
		fi
		# cross: 5.1-target output must also run under luau
		if [[ "$d" == "5.1" ]]; then
			c3="$(check "$base" "$LUAU" "$out")"; rc3=$?
			c1l="$(check "$base" "$LUAU" "$case")"; rc1l=$?
			if [[ "$rc3" != "$rc1l" || "$c3" != "$c1l" ]]; then
				echo "FAIL [cross] $base (5.1out on luau)"
				fail=$((fail + 1)); failed+=("$base/cross")
			else
				pass=$((pass + 1))
			fi
		fi
		# luau-compile syntax check for luau targets (binary to /dev/null)
		if [[ "$d" == "luau" ]]; then
			if ! timeout 30 "$LUAUC" "$out" > /dev/null 2>"$TMP/luac_err.txt"; then
				echo "FAIL [luac] $base: $(head -1 "$TMP/luac_err.txt")"
				fail=$((fail + 1)); failed+=("$base/luac")
			else
				pass=$((pass + 1))
			fi
		fi
	done
done

echo "=================================================="
echo "PASS checks: $pass   FAIL: $fail"
if [[ $fail -gt 0 ]]; then
	for f in "${failed[@]}"; do
		echo "  failed: $f"
	done
	exit 1
fi
echo "ALL GREEN ✓"
