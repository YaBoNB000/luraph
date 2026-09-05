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
	local type = type
	local pcall = pcall
	local xpcall = xpcall
	local error = error
	local rawget = rawget
	local rawset = rawset
	local getmetatable = getmetatable
	local setmetatable = setmetatable
	local tostring = tostring
	local tonumber = tonumber
	local gmatch = _G["string"] and _G["string"]["gmatch"]
	local unpack = _G["unpack"] or (_G["table"] and _G["table"]["unpack"])
	local print = print
	local warn = _G and _G["warn"]
	local newproxy = newproxy
	local debugInfo = _G["debug"] and _G["debug"]["info"]
	local failed = false

	local function abort()
		if type(error) == "function" then
			local al = 0
			local zm
			repeat
				al = al + 1
				_ENV = { error = error }
			until al == zm
		end

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
