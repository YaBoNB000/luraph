#!/bin/bash
# M0+ test matrix (mandatory workflow):
# every corpus case x every applicable dialect:
#   1. tool must succeed
#   2. obfuscated output must run with IDENTICAL stdout+exit as the original
#      (under the dialect's interpreter)
#   3. 5.1-target output must also run identically under Luau
#   4. luau-target output must pass luau-compile syntax check
# A timeout (exit 124) is always a FAIL (the corpus must terminate).
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TOOL="$ROOT/target/release/luraph-rs"
# toolchain: prefer /home/user/tools/bin; fall back to the in-repo
# .tools/bin (survives sandbox resets that wipe /home/user/tools)
TOOLS_BIN=/home/user/tools/bin
if [ ! -x "$TOOLS_BIN/lua51" ] || [ ! -x "$TOOLS_BIN/luau-compile" ]; then
	# repo-root .tools (survives sandbox resets that wipe /home/user/tools)
	TOOLS_BIN="$(cd "$(dirname "$0")/../../.tools/bin" && pwd)"
fi
LUA51="$TOOLS_BIN/lua51"
LUAU="$TOOLS_BIN/luau"
LUAUC="$TOOLS_BIN/luau-compile"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

pass=0
fail=0
failed=()

run() { # run <interp> <file>  -> stdout+stderr; returns exit code
	timeout 30 "$1" "$2" 2>&1
}

for case in "$ROOT"/tests/cases/*.lua; do
	base="$(basename "$case" .lua)"
	if [[ "$base" == luau_* ]]; then
		dialects="luau"
	else
		dialects="5.1 luau"
	fi
	for d in $dialects; do
		out="$TMP/${base}__${d}.lua"
		if ! "$TOOL" --dialect "$d" --seed 42 "$case" "$out" 2>"$TMP/tool_err.txt"; then
			echo "FAIL [tool]  $base ($d): $(head -1 "$TMP/tool_err.txt")"
			fail=$((fail + 1)); failed+=("$base/$d/tool")
			continue
		fi
		if [[ "$d" == "5.1" ]]; then
			interp=$LUA51
		else
			interp=$LUAU
		fi
		o1="$(run "$interp" "$case")"; c1=$?
		o2="$(run "$interp" "$out")"; c2=$?
		if [[ "$c1" != "$c2" || "$o1" != "$o2" ]]; then
			echo "FAIL [run]   $base ($d): exit $c1 vs $c2"
			diff <(echo "$o1") <(echo "$o2") | head -6
			fail=$((fail + 1)); failed+=("$base/$d/run")
		else
			pass=$((pass + 1))
		fi
		# cross: 5.1-target output must also run identically under luau
		if [[ "$d" == "5.1" ]]; then
			c3="$(run "$LUAU" "$out")"; rc3=$?
			c1l="$(run "$LUAU" "$case")"; rc1l=$?
			if [[ "$rc3" != "$rc1l" || "$c3" != "$c1l" ]]; then
				echo "FAIL [cross] $base (5.1out on luau): exit $rc1l vs $rc3"
				diff <(echo "$c1l") <(echo "$c3") | head -6
				fail=$((fail + 1)); failed+=("$base/cross")
			else
				pass=$((pass + 1))
			fi
		fi
		# luau-compile syntax check for luau targets
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

# ---------------------------------------------------------------
# VM phase (M4/L6): same matrix with --vm (private bytecode +
# generated obfuscated interpreter). VM containers are large and
# run a Lua-on-Lua interpreter, so timeouts are 120s.
# ---------------------------------------------------------------
runvm() { # runvm <interp> <file>
	timeout 120 "$1" "$2" 2>&1
}

for case in "$ROOT"/tests/cases/*.lua; do
	base="$(basename "$case" .lua)"
	if [[ "$base" == luau_* ]]; then
		dialects="luau"
	else
		dialects="5.1 luau"
	fi
	for d in $dialects; do
		out="$TMP/${base}__${d}.vm.lua"
		if ! "$TOOL" --vm --dialect "$d" --seed 42 "$case" "$out" 2>"$TMP/tool_err.txt"; then
			echo "FAIL [vm:tool]  $base ($d): $(head -1 "$TMP/tool_err.txt")"
			fail=$((fail + 1)); failed+=("$base/$d/vm/tool")
			continue
		fi
		if [[ "$d" == "5.1" ]]; then
			interpret=$LUA51
		else
			interpret=$LUAU
		fi
		o1v="$(runvm "$interpret" "$case")"; c1v=$?
		o2v="$(runvm "$interpret" "$out")"; c2v=$?
		if [[ "$c1v" != "$c2v" || "$o1v" != "$o2v" ]]; then
			echo "FAIL [vm:run]   $base ($d): exit $c1v vs $c2v"
			diff <(echo "$o1v") <(echo "$o2v") | head -6
			fail=$((fail + 1)); failed+=("$base/$d/vm/run")
		else
			pass=$((pass + 1))
		fi
		# cross: 5.1-target VM output must also run identically under luau
		if [[ "$d" == "5.1" ]]; then
			c3v="$(runvm "$LUAU" "$out")"; rc3v=$?
			c1lv="$(runvm "$LUAU" "$case")"; rc1lv=$?
			if [[ "$rc3v" != "$rc1lv" || "$c3v" != "$c1lv" ]]; then
				echo "FAIL [vm:cross] $base (5.1out on luau): exit $rc1lv vs $rc3v"
				diff <(echo "$c1lv") <(echo "$c3v") | head -6
				fail=$((fail + 1)); failed+=("$base/$d/vm/cross")
			else
				pass=$((pass + 1))
			fi
		fi
		# luau-compile syntax check for luau-target VM output
		if [[ "$d" == "luau" ]]; then
			if ! timeout 60 "$LUAUC" "$out" > /dev/null 2>"$TMP/luac_err.txt"; then
				echo "FAIL [vm:luac] $base: $(head -1 "$TMP/luac_err.txt")"
				fail=$((fail + 1)); failed+=("$base/$d/vm/luac")
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
echo "ALL GREEN"
