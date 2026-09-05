//! Anti-debug stage modules (suggestion 2, 2026-09-05).
//!
//! Each anti-debug check is an independent stage source snippet. The
//! guard assembler (guard.rs) composes a random ORDERING of the stages
//! per build, so the anti-debug sequence differs build-to-build
//! (anti-folder: anti1/anti3-style modules, randomized per build).
//! Every stage is always present (strong baseline protection); only the
//! ordering is randomized, which defeats a fixed single-step bypass
//! without weakening any build.
//!
//! Each stage is a `local function stage_X() ... end` that closes over
//! the guard preamble's upvalues (type/pcall/error/failed/abort/
//! tripwire/canaries/env).

/// Stage 1 — core globals must be real functions.
pub const ANTI_CORE: &str = r###"	local function stage_core()
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
"###;

/// Stage 2 — environment sanity (getfenv/_G, negative-key roundtrip,
/// pcall(error) must raise).
pub const ANTI_ENV: &str = r###"	local function stage_env()
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
"###;

/// Stage 3 — debug.info line probe + loader-hook (load/loadstring)
/// native-source detection.
pub const ANTI_DEBUG: &str = r###"	local function stage_debug()
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
"###;

/// Stage 4 — newproxy metatable tripwires + table canaries with locked
/// __metatable.
pub const ANTI_CANARY: &str = r###"	local function stage_canaries()
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
"###;

/// Stage 5 — unpack probe + env print/warn identity.
pub const ANTI_MISC: &str = r###"	local function stage_misc()
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
"###;

/// (stage name, source, call-line) for the assembler.
pub const STAGES: [(&str, &str, &str); 5] = [
	("core", ANTI_CORE, "stage_core()"),
	("env", ANTI_ENV, "stage_env()"),
	("debug", ANTI_DEBUG, "stage_debug()"),
	("canary", ANTI_CANARY, "stage_canaries()"),
	("misc", ANTI_MISC, "stage_misc()"),
];
