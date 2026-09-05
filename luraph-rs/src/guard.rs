//! Anti-debug / environment-integrity guard (user-provided design,
//! 2026-08-29): injected as a prelude in front of the obfuscated
//! payload on the standard pipeline (v15 keeps its 3-line form; a
//! CHAR-encoded v15 variant is a future increment).
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
//!   - loader hook integrity (suggestion 3, 2026-08-29): loadstring /
//!     load present but non-native (debug source ~= "[C]") => hooked
//!   - newproxy metatable canaries (__tostring/__concat/__call
//!     tripwires) + table canaries with locked __metatable
//!   - unpack({}, 0, 64) must succeed
//!
//! On any failed check `abort()` hangs/poisons the environment
//! (`_ENV` swap + nil-index) instead of revealing the payload.
//!
//! The prelude goes through mangle + minify so its internals carry
//! build-random names; globals it references are untouched by mangle.

use crate::mangle;
use crate::minify;
use crate::parser;
use crate::printer;
use crate::rng::Rng;

/// The guard IIFE (verbatim user design; the trailing `print(255)`
/// test harness is NOT included — only `local _guard = (...)`).
const GUARD_SRC: &str = r###"local _guard = (function()
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
	local gmatch = string and string.gmatch
	local unpack = unpack or (table and table.unpack)
	local print = print
	local warn = _G and _G.warn
	local newproxy = newproxy
	local debugInfo = debug and debug.info
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

	local function stage_core()
		if
			type(type) ~= "function"
			or type(pcall) ~= "function"
			or type(xpcall) ~= "function"
			or type(error) ~= "function"
			or type(rawget) ~= "function"
			or type(rawset) ~= "function"
			or type(getmetatable) ~= "function"
			or type(setmetatable) ~= "function"
		then
			abort()
		end
	end

	local env

	local function stage_env()
		if type(getfenv) == "function" then
			local ok, value = pcall(getfenv)

			if ok then
				env = value
			end
		end

		if env == nil then
			env = _G
		end

		if type(env) ~= "table" or type(print) ~= "function" or (warn ~= nil and type(warn) ~= "function") then
			failed = true
		end

		local slot = {}
		local marker = function() end
		local writeOk = pcall(rawset, slot, -271823, marker)

		if not writeOk or rawget(slot, -271823) ~= marker then
			failed = true
		end

		rawset(slot, -271823, nil)

		if rawget(slot, -271823) ~= nil or pcall(error) then
			failed = true
		end
	end

	local function stage_debug()
		local function lineProbe() return (nil)[1] end

		if
			type(debugInfo) == "function"
			and type(gmatch) == "function"
			and type(tostring) == "function"
			and type(tonumber) == "function"
		then
			local lineOk, line = pcall(debugInfo, lineProbe, "l")
			local sourceOk, source = pcall(debugInfo, lineProbe, "s")
			local errorLine
			local callOk = xpcall(lineProbe, function(message)
				for digits in gmatch(tostring(message), ":(%d+):") do
					errorLine = tonumber(digits)
				end

				return message
			end)

			if
				callOk
				or not lineOk
				or type(line) ~= "number"
				or not sourceOk
				or type(source) ~= "string"
				or source == "[C]"
				or type(errorLine) ~= "number"
				or line ~= errorLine
			then
				failed = true
			end

			local nativeOk, nativeSource = pcall(debugInfo, error, "s")

			if nativeOk and type(nativeSource) == "string" and nativeSource ~= "[C]" then
				failed = true
			end
		end

		if type(debugInfo) == "function" then
			local function isNative(f)
				if type(f) ~= "function" then
					return false
				end
				local ok, src = pcall(debugInfo, f, "s")
				return ok and type(src) == "string" and src == "[C]"
			end

			local envTable = _G

			if type(envTable) == "table" then
				local loaderLS = rawget(envTable, "loadstring")
				local loaderL = rawget(envTable, "load")

				if type(loaderLS) == "function" and not isNative(loaderLS) then
					failed = true
				end

				if type(loaderL) == "function" and not isNative(loaderL) then
					failed = true
				end
			end
		end
	end

	local function stage_canaries()
		if type(newproxy) == "function" then
			local proxyOk, proxy = pcall(newproxy, true)

			if not proxyOk or proxy == nil then
				failed = true
			else
				local metatableOk, metatable = pcall(getmetatable, proxy)

				if not metatableOk or type(metatable) ~= "table" then
					failed = true
				else
					metatable.__tostring = tripwire
					metatable.__concat = tripwire
					metatable.__call = tripwire
					canaries[#canaries + 1] = proxy
				end
			end
		end

		local function addCanary(lock)
			local value = {}
			local metatable = {
				__metatable = lock,
				__tostring = tripwire,
				__concat = tripwire,
				__iter = tripwire,
			}
			local ok, result = pcall(setmetatable, value, metatable)

			if not ok or result ~= value then
				failed = true
				return
			end

			local readOk, visibleMetatable = pcall(getmetatable, value)

			if not readOk or visibleMetatable ~= lock then
				failed = true
			end

			canaries[#canaries + 1] = value
		end

		addCanary("tddxhpfcpi")
		addCanary("lsphhfstdm")
		addCanary("vymgglkhqr")
	end

	local function stage_misc()
		if type(unpack) == "function" then
			local ok = pcall(unpack, {}, 0, 64)

			if not ok then
				failed = true
			end
		end

		if type(env) == "table" then
			if type(print) == "function" and env.print ~= print then
				failed = true
			end

			if type(warn) == "function" and env.warn ~= warn then
				failed = true
			end
		end
	end

	stage_core()
	stage_env()
	stage_debug()
	stage_canaries()
	stage_misc()

	if failed then
		abort()
	end

	return canaries
end)()"###;

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
	let strings = lua_string_literals(GUARD_SRC);
	let mut gs = String::from("\nlocal GS={}\nlocal schar=string and string.char\nif schar then\n");
	for (k, s) in strings.iter().enumerate() {
		let codes: Vec<String> = s.bytes().map(|c| c.to_string()).collect();
		gs.push_str(&format!("GS[{}]=schar({})\n", k + 1, codes.join(",")));
	}
	gs.push_str("end\n");
	// mangle the guard first (build-random names), then swap the string
	// literals for GS[k] and splice the GS table in -- mangle leaves
	// string literals untouched, so the swap stays exact.
	let mangled = match parser::parse(GUARD_SRC, true).ok() {
		Some(mut block) => {
			let mut table = crate::symtab::resolve(&mut block);
			mangle::mangle(&mut table, rng, false);
			printer::print_chunk_luau(&table, &block)
		}
		None => GUARD_SRC.to_string(),
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
	let parsed = parser::parse(GUARD_SRC, luau).ok();
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
	GUARD_SRC.to_string()
}
