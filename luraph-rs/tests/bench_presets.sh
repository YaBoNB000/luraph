#!/bin/bash
# M6 performance snapshot. Prints a markdown table to stdout.
# Measures obfuscate wall time, output bytes, and run wall time for a
# small representative corpus under every named preset.
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TOOL="$ROOT/target/release/luraph-rs"
TOOLS_BIN=/home/user/tools/bin
if [ ! -x "$TOOLS_BIN/lua51" ]; then
	TOOLS_BIN="$(cd "$(dirname "$0")/../../.tools/bin" && pwd)"
fi
LUA51="$TOOLS_BIN/lua51"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

CASES="basics functions game_loop tables stress_bigtable"
PRESETS="low medium high vm max"

python3 - "$TOOL" "$LUA51" "$ROOT" "$TMP" $CASES <<'PY'
import sys, subprocess, time, os
tool, lua51, root, tmp = sys.argv[1:5]
cases = sys.argv[5:]
presets = ["low", "medium", "high", "vm", "max"]

def wall(cmd, timeout=120):
    t0 = time.perf_counter()
    p = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=timeout)
    dt = time.perf_counter() - t0
    return dt, p.returncode, p.stdout

print("| 语料 | 预设 | 混淆 ms | 输出字节 | 原脚本 ms | 混淆后 ms | 减速 |")
print("|---|---|---:|---:|---:|---:|---:|")
for case in cases:
    src = os.path.join(root, "tests", "cases", f"{case}.lua")
    orig_dt, orig_rc, orig_out = wall([lua51, src], timeout=30)
    for preset in presets:
        out = os.path.join(tmp, f"{case}.{preset}.lua")
        obf_dt, obf_rc, _ = wall(
            [tool, "--preset", preset, "--dialect", "5.1", "--seed", "42", src, out],
            timeout=90,
        )
        size = os.path.getsize(out) if os.path.isfile(out) else 0
        run_dt, run_rc, run_out = wall([lua51, out], timeout=120)
        slow = (run_dt / orig_dt) if orig_dt > 1e-6 else float("inf")
        print(
            f"| `{case}` | `{preset}` | {obf_dt*1000:.0f} | {size} | "
            f"{orig_dt*1000:.1f} | {run_dt*1000:.1f} | {slow:.1f}× |"
        )
        sys.stdout.flush()
PY
