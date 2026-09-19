//! Anti-debug / environment-integrity guard (user-provided design,
//! 2026-08-29): injected as a prelude in front of the obfuscated
//! payload on the standard pipeline; v15 injects a CHAR-encoded copy at
//! the head of the FC entry machine (keeps the 3-line form).
//!
//! Suggestion 2 (2026-09-05): the guard's five check stages live in
//! `src/anti/mod.rs` as independent stage modules (anti1/anti3 style).
//! `assemble_guard` composes all five stages in a RANDOM ORDER per
//! build (all stages always present, only ordering varies), so the
//! anti-debug sequence differs build-to-build and a fixed single-step
//! bypass cannot be reused.
//!
//! The guard is a self-contained IIFE assigned to a fresh local. It
//! verifies the runtime environment before the payload executes:
//!
//!   - core globals are real functions (type/pcall/xpcall/error/raw*)
//!   - getfenv/_G environment sanity + print/warn identity in env
//!   - table write/read roundtrip with a negative key
//!   - pcall(error) must FAIL (error must raise)
//!   - debug.info line probe: a probe function's reported line must
//!     equal the line extracted from its own raised error message;
//!     debug info source of `error` must stay "[C]"
//!   - loader hook integrity (suggestion 3): loadstring / load present
//!     but non-native (debug source ~= "[C]") => hooked; re-checked
//!     mid-staging in the verify handler too
//!   - newproxy metatable canaries (__tostring/__concat/__call
//!     tripwires) + table canaries with locked __metatable
//!   - unpack({}, 0, 64) must succeed
//!
//! On any failed check `abort()` hangs/poisons the environment
//! (`_ENV` swap + nil-index) instead of revealing the payload.
//!
//! The prelude goes through mangle + minify so its internals carry
//! build-random names; globals it references are untouched by mangle.

use crate::anti;
use crate::mangle;
use crate::minify;
use crate::parser;
use crate::printer;
use crate::rng::Rng;

/// The guard IIFE (verbatim user design; the trailing `print(255)`
/// test harness is NOT included — only `local _guard = (...)`).
const GUARD_PREAMBLE: &str = r###"local _guard = (function()
	-- ㊶: EV = 可见引导以裸全局引用捕获的应用环境全局表（setfenv 影子
	-- 环境里的值）。守卫位于 HBOOT 掩码层（标准环境加载），必须经 EV
	-- 才能看到攻击者的影子环境替换——EV 缺省(旧管线)时退回裸全局。
	-- EV 索引: 1=type 2=pcall 3=xpcall 4=error 5=rawget 6=rawset
	-- 7=getmetatable 8=setmetatable 9=tostring 10=tonumber 11=string
	-- 12=unpack 13=print 14=warn 15=newproxy 16=debug 17=getfenv 18=_G
	local type = (EV and EV[1]) or type
	local pcall = (EV and EV[2]) or pcall
	local xpcall = (EV and EV[3]) or xpcall
	local error = (EV and EV[4]) or error
	local rawget = (EV and EV[5]) or rawget
	local rawset = (EV and EV[6]) or rawset
	local getmetatable = (EV and EV[7]) or getmetatable
	local setmetatable = (EV and EV[8]) or setmetatable
	local tostring = (EV and EV[9]) or tostring
	local tonumber = (EV and EV[10]) or tonumber
	local gmatch = (EV and EV[11] and EV[11]["gmatch"]) or (_G["string"] and _G["string"]["gmatch"])
	local unpack = (EV and EV[12]) or _G["unpack"] or (_G["table"] and _G["table"]["unpack"])
	local print = (EV and EV[13]) or print
	local warn = (EV and EV[14]) or (_G and _G["warn"])
	local newproxy = (EV and EV[15]) or newproxy
	local debugInfo = (EV and EV[16] and EV[16]["info"]) or (_G["debug"] and _G["debug"]["info"])
	local failed = false

	local function abort()
		-- ㊶: 纯挂起形态（_ENV 投毒在只读全局环境会抛错破坏挂起语义；
		-- 挂起本身已是终止——守卫现在位于 HBOOT 掩码层，形态不可见）。
		local al = 0
		local zm
		repeat
			al = al + 1
		until al == zm

		return (nil)[1]
	end

	local function tripwire()
		failed = true
		abort()
	end

	local canaries = {}

	local env

"###;

const GUARD_EPILOGUE: &str = r###"
	if failed then
		abort()
	end

	return canaries
end)()"###;

/// Assemble the guard IIFE from the anti/ stage modules (suggestion 2,
/// anti-folder). All five stages are always present (strong baseline
/// protection); only their ORDER is randomized per build, so the
/// anti-debug sequence differs build-to-build and a fixed single-step
/// bypass cannot be reused.
fn assemble_guard(rng: &mut Rng) -> String {
	let mut order: Vec<usize> = (0..anti::STAGES.len()).collect();
	rng.shuffle(&mut order);
	let mut src = String::from(GUARD_PREAMBLE);
	for &i in &order {
		src.push_str(anti::STAGES[i].1);
		src.push('\n');
	}
	src.push('\n');
	for &i in &order {
		src.push('\t');
		src.push_str(anti::STAGES[i].2);
		src.push('\n');
	}
	src.push_str(GUARD_EPILOGUE);
	src
}


/// Extract the unique double-quoted string literals of a Lua source,
/// in order of first appearance (the guard sources contain no escaped
/// quotes, so a plain quote scan is exact).
fn lua_string_literals(src: &str) -> Vec<String> {
	let b = src.as_bytes();
	let mut out: Vec<String> = Vec::new();
	let mut i = 0usize;
	while i < b.len() {
		if b[i] == b'"' {
			let start = i + 1;
			let mut j = start;
			while j < b.len() && b[j] != b'"' {
				j += 1;
			}
			let s = std::str::from_utf8(&b[start..j]).unwrap_or("").to_string();
			if !out.contains(&s) {
				out.push(s);
			}
			i = j + 1;
		} else {
			i += 1;
		}
	}
	out
}

/// Replace every double-quoted literal with `GS[k]` (k = index in the
/// dedup list). ASCII source -> byte walk is exact.
fn replace_literals_with_table(src: &str, strings: &[String]) -> String {
	let b = src.as_bytes();
	let mut out = String::with_capacity(src.len());
	let mut i = 0usize;
	while i < b.len() {
		if b[i] == b'"' {
			let start = i + 1;
			let mut j = start;
			while j < b.len() && b[j] != b'"' {
				j += 1;
			}
			let s = std::str::from_utf8(&b[start..j]).unwrap_or("");
			let k = strings.iter().position(|x| x == s).unwrap_or(0) + 1;
			out.push_str(&format!("GS[{k}]"));
			i = j + 1;
		} else {
			out.push(b[i] as char);
			i += 1;
		}
	}
	out
}

/// v15 variant of the guard (F10-safe): every string literal is
/// rebuilt from numeric char codes into a local `GS` table inside the
/// IIFE, so the injected guard contributes ZERO visible string
/// literals to the v15 output (32/32 fingerprints stay intact). If
/// `string.char` is unavailable (hooked env), the GS table stays empty
/// and every integrity comparison fails -> abort() fires naturally.
/// The guard is mangled so its local/stage function names are
/// build-random (the scaffold module table is otherwise built after
/// mangle and would leave them readable). The result is a
/// `local _guard=(function()...end)()` statement ready to prepend to
/// the FC entry-machine body.
pub fn v15_guard_source(rng: &mut Rng) -> String {
	let guard_src = assemble_guard(rng);
	let strings = lua_string_literals(&guard_src);
	let mut gs = String::from("\nlocal GS={}\nlocal schar=string and string.char\nif schar then\n");
	for (k, s) in strings.iter().enumerate() {
		let codes: Vec<String> = s.bytes().map(|c| c.to_string()).collect();
		gs.push_str(&format!("GS[{}]=schar({})\n", k + 1, codes.join(",")));
	}
	gs.push_str("end\n");
	// mangle the guard first (build-random names), then swap the string
	// literals for GS[k] and splice the GS table in -- mangle leaves
	// string literals untouched, so the swap stays exact.
	let mangled = match parser::parse(&guard_src, true).ok() {
		Some(mut block) => {
			let mut table = crate::symtab::resolve(&mut block);
			// the spliced-in GS table + schar alias keep their literal
			// names (inserted after mangle) — mangle must never draw
			// those exact names for its own locals (a mangled stage
			// function named "GS" would shadow the injected table and
			// turn GS[k] into indexing a function)
			table.globals.push("GS".to_string());
			table.globals.push("schar".to_string());
			mangle::mangle(&mut table, rng, false);
			printer::print_chunk_luau(&table, &block)
		}
		None => guard_src.clone(),
	};
	let replaced = replace_literals_with_table(&mangled, &strings);
	let marker = "(function()";
	let pos = replaced.find(marker).expect("guard IIFE marker") + marker.len();
	format!("{}{}{}", &replaced[..pos], gs, &replaced[pos..])
}

/// ㊶ (保护代码隐藏, 对照 main 样本 15 的形态): HBOOT 掩码层嵌入版守卫。
/// 守卫整体搬进 HBOOT 源码（元钥匙流掩码字节 ⇒ 文件中天然不可见），
/// 可见层从此对保护代码零痕迹。字符串字面量**保持原样**——掩码层已提供
/// 不可见性，数字编码（GS/schar 码表）只在可见层才需要，那里是「纸老虎」
/// （任何分析者都能解码数字表）。返回 (守卫语句, 持有局部名)：守卫 IIFE
/// 返回 canaries 蜜罐表，由持有局部接住、经 HBOOT 第 5 返回值递出、
/// 由可见引导存进运行期状态（不透明表值，形似 VM 状态）。
pub fn hboot_guard_source(rng: &mut Rng) -> (String, String) {
	let guard_src = assemble_guard(rng);
	let mangled = match parser::parse(&guard_src, true).ok() {
		Some(mut block) => {
			let mut table = crate::symtab::resolve(&mut block);
			mangle::mangle(&mut table, rng, false);
			printer::print_chunk_luau(&table, &block)
		}
		None => guard_src.clone(),
	};
	// 守卫形态固定为 `local NAME = (function() ... end)()`——取 NAME。
	let trimmed = mangled.trim_start();
	let holder = trimmed
		.strip_prefix("local ")
		.and_then(|r| r.split('=').next())
		.map(|s| s.trim().to_string())
		.unwrap_or_else(|| "_hp".to_string());
	(mangled, holder)
}

/// Build the guard prelude: parse -> mangle -> print -> minify, so the
/// guard ships with build-random local names and compact form. Returns
/// the ready-to-prepend statement text (single line, no trailing
/// newline). On any internal failure returns the verbatim source
/// (protection must never silently disappear).
pub fn guard_prelude(rng: &mut Rng, luau: bool) -> String {
	let guard_src = assemble_guard(rng);
	let parsed = parser::parse(&guard_src, luau).ok();
	if let Some(mut block) = parsed {
		let mut table = crate::symtab::resolve(&mut block);
		mangle::mangle(&mut table, rng, false);
		let printed = if luau {
			printer::print_chunk_luau(&table, &block)
		} else {
			printer::print_chunk(&table, &block)
		};
		match minify::minify(&printed, luau) {
			Ok(m) => return m.trim_end_matches('\n').to_string(),
			Err(_) => {}
		}
	}
	guard_src
}
