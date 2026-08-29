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

	local env

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

	local canaries = {}

	local function tripwire()
		failed = true
		abort()
	end

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

	if failed then
		abort()
	end

	return canaries
end)()"###;

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
