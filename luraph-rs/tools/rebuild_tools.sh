#!/usr/bin/env bash
# 一键重建 .tools 工具链（沙箱重置后运行）。
# 步骤 = HANDOFF §4：Rust 1.88(@rustbin) + Lua 5.1.5(GitHub 镜像) +
# Luau 0.735(g++ 直编，自写 main 复刻官方 CLI：loadstring/require rehook/
# luaL_sandbox)。全部落在仓库内 /home/user/luraph/.tools/（gitignored）。
set -euo pipefail
T=/home/user/luraph/.tools
mkdir -p "$T/bin" "$T/lib"
cd /tmp

# 1) Rust 1.88（npm @rustbin；每个包解到独立目录，合入同一目录会互相覆盖）
if [ ! -x "$T/bin/rustc" ]; then
	for p in rustc cargo rust-std; do
		npm pack @rustbin/$p-1.88.0-x86_64-unknown-linux-gnu >/dev/null 2>&1 || true
		rm -rf x_$p && mkdir -p x_$p
		tar xzf rustbin-$p-*.tgz -C x_$p
		mkdir -p "$T/lib/$p"
		cp -r x_$p/package/. "$T/lib/$p/"
	done
	cp -rn "$T/lib/rust-std/rust-std-x86_64-unknown-linux-gnu/lib/rustlib/x86_64-unknown-linux-gnu/." \
		"$T/lib/rustc/rustc/lib/rustlib/x86_64-unknown-linux-gnu/"
	ln -sf ../lib/rustc/rustc/bin/rustc "$T/bin/rustc"
	ln -sf ../lib/cargo/cargo/bin/cargo "$T/bin/cargo"
	echo "[ok] rust 1.88"
fi

# 2) Lua 5.1.5（lua.org 不可达 → GitHub 镜像；无 readline → make generic）
if [ ! -x "$T/bin/lua51" ]; then
	curl -sL -o l.tar.gz https://codeload.github.com/zgpxgame/lua-5.1.5/tar.gz/refs/heads/master
	rm -rf lua-5.1.5-master && tar xzf l.tar.gz
	make -C lua-5.1.5-master/src generic MYLDFLAGS="-ldl -lm" -j4 >/dev/null
	cp lua-5.1.5-master/src/lua "$T/bin/lua51"
	cp lua-5.1.5-master/src/luac "$T/bin/luac51"
	echo "[ok] lua 5.1.5"
fi

# 3) Luau 0.735（无 cmake → g++ 直编 7 库 + 仓库内自写 main）
if [ ! -x "$T/bin/luau" ]; then
	curl -sL -o luau.tar.gz https://codeload.github.com/luau-lang/luau/tar.gz/refs/tags/0.735
	rm -rf luau-0.735 && tar xzf luau.tar.gz
	cd luau-0.735
	cp /home/user/luraph/luraph-rs/tools/luau-cli-mains/main_luau*.cpp .
	SRC=$(ls VM/src/*.cpp Common/src/*.cpp Ast/src/*.cpp Bytecode/src/*.cpp \
		Compiler/src/*.cpp Require/src/*.cpp Config/src/*.cpp)
	g++ -O2 -std=c++17 -DLUA_USE_LONGJMP=1 '-DLUA_API=extern "C"' \
		-I VM/include -I Common/include -I Ast/include -I Bytecode/include \
		-I Compiler/include -I Require/include -I Config/include \
		main_luau.cpp $SRC -o "$T/bin/luau" -pthread
	SRC=$(ls VM/src/*.cpp Common/src/*.cpp Ast/src/*.cpp Bytecode/src/*.cpp \
		Compiler/src/*.cpp)
	g++ -O2 -std=c++17 -DLUA_USE_LONGJMP=1 '-DLUA_API=extern "C"' \
		-I VM/include -I Common/include -I Ast/include -I Bytecode/include \
		-I Compiler/include \
		main_luau_compile.cpp $SRC -o "$T/bin/luau-compile" -pthread
	cd /tmp
	echo "[ok] luau 0.735"
fi

"$T/bin/rustc" --version
"$T/bin/lua51" -v
echo "toolchain ready: $T/bin"
