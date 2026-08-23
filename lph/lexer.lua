-- lph/lexer.lua
-- Tokenizer for Lua 5.1 and Luau source.
-- Emits a flat token list; the parser does lookahead on it.
-- NOTE: this file must stay valid plain Lua 5.1 (no goto, no //, no bitops).

local Lex = {}

local KEYWORDS = {
	and = true, break = true, do = true, else = true, elseif = true, end = true,
	false = true, for = true, function = true, if = true, in = true, local = true,
	nil = true, not = true, or = true, repeat = true, return = true, then = true,
	true = true, until = true, while = true,
}

local function is_digit(c) return c >= "0" and c <= "9" end
local function is_hex(c)
	return is_digit(c) or (c >= "a" and c <= "f") or (c >= "A" and c <= "F")
end
local function is_alpha(c)
	return (c >= "a" and c <= "z") or (c >= "A" and c <= "Z") or c == "_"
end

-- two-char / three-char punct (checked before single-char, longest first)
local TWO = { "...", "..", "==", "~=", "<=", ">=", "//", "+=", "-=", "*=", "/=", "%=", "^=" }

local SINGLE = "={}()<>+-*/%^#%:,.;&?"

local function new(src, opts)
	opts = opts or {}
	local pos, line, n = 1, 1, #src
	local toks = {}

	local function err(msg)
		error(string.format("lex error line %d: %s", line, msg), 0)
	end

	local function adv(k)
		for i = 1, (k or 1) do
			if src:sub(pos + i - 1, pos + i - 1) == "\n" then
				line = line + 1
			end
		end
		pos = pos + (k or 1)
	end

	local function tok(k, v)
		local t = { k = k, v = v, line = line }
		toks[#toks + 1] = t
		return t
	end

	local function skip_ws_and_comments()
		while pos <= n do
			local c = src:sub(pos, pos)
			if c == " " or c == "\t" or c == "\r" or c == "\n" then
				adv(1)
			elseif c == "-" and src:sub(pos + 1, pos + 1) == "-" then
				local p = pos + 2
				local lvl = 0
				while src:sub(p + lvl, p + lvl) == "=" do
					lvl = lvl + 1
					p = p + 1
				end
				if src:sub(p, p) == "[" then
					local close = "]" .. string.rep("=", lvl) .. "]"
					local q = src:find(close, p + 1, true)
					if not q then err("unterminated comment") end
					for _ in src:sub(pos, q + #close - 1):gmatch("\n") do
						line = line + 1
					end
					pos = q + #close
				else
					local q = src:find("\n", pos, true)
					if q then pos = q - 1 else pos = n + 1 end
				end
			else
				break
			end
		end
	end

	local function count_newlines(s)
		local c = 0
		for _ in s:gmatch("\n") do c = c + 1 end
		return c
	end

	local function lex_number()
		local s = src:sub(pos, n)
		local body, exp
		if s:sub(1, 1) == "0" and (s:sub(2, 2) == "x" or s:sub(2, 2) == "X") then
			body, exp = s:match("^0[xX]([%x]+)(.*)$")
		else
			body, exp = s:match("^(%.?[%d]+%.?)([eE][+-]?[%d]+)?(.*)$")
		end
		if not body or not body:match("[%d]") then err("malformed number") end
		local val = tonumber(body .. (exp or ""))
		if not val then err("malformed number") end
		local t = tok("num", val)
		t.isfloat = (body:match("[%.eE]") ~= nil)
		t.raw = body .. (exp or "")
		adv(#body + (exp and #exp or 0))
		return t
	end

	local function line_cont()
		-- pos is right after the backslash (backslash already advanced).
		-- skip the newline plus any following whitespace / single-line comments
		while pos <= n do
			local c = src:sub(pos, pos)
			if c == "\n" then
				adv(1)
				break
			elseif c == " " or c == "\t" or c == "\r" then
				adv(1)
			elseif c == "-" and src:sub(pos + 1, pos + 1) == "-" then
				local q = src:find("\n", pos, true)
				if not q then
					pos = n + 1
					break
				end
				pos = q
			else
				break
			end
		end
	end

	local function lex_short_string(quote)
		adv(1) -- opening quote
		local out = {}
		while true do
			if pos > n then err("unterminated string") end
			local c = src:sub(pos, pos)
			if c == quote then
				adv(1)
				return tok("str", table.concat(out))
			elseif c == "\n" then
				err("unterminated string (newline)")
			elseif c == "\\" then
				local e = src:sub(pos + 1, pos + 1)
				local ch
				if e == "\n" then
					adv(1) -- the backslash
					line_cont()
				elseif e == "" then
					err("unterminated string escape")
				elseif e == "n" or e == "t" or e == "r" or e == "a" or e == "b"
					or e == "f" or e == "v" or e == "\\" or e == '"' or e == "'" then
					ch = (e == "n") and "\n" or (e == "t") and "\t" or (e == "r") and "\r"
						or (e == "a") and "\a" or (e == "b") and "\b" or (e == "f") and "\f"
						or (e == "v") and "\v" or e
					adv(2)
					table.insert(out, ch)
				elseif is_digit(e) then
					local d = src:match("^\\([%d][%d]?[%d]?)", pos)
					local num = tonumber(d)
					if num == nil or num > 255 then err("invalid escape value") end
					adv(1 + #d)
					table.insert(out, string.char(num))
				elseif e == "x" then
					local h = src:match("^\\(%x%x?)", pos)
					adv(1 + #h)
					table.insert(out, string.char(tonumber(h:sub(2, 2), 16)))
				else
					err("invalid escape sequence")
				end
			else
				table.insert(out, c)
				adv(1)
			end
		end
	end

	local function lex_long_string()
		local p = pos
		local lvl = 0
		while src:sub(p + 1 + lvl, p + 1 + lvl) == "=" do
			lvl = lvl + 1
			p = p + 1
		end
		local open = "[" .. string.rep("=", lvl) .. "["
		local close = "]" .. string.rep("=", lvl) .. "]"
		if src:sub(pos, pos + #open - 1) ~= open then err("bad long string") end
		local q = src:find(close, pos + #open, true)
		if not q then err("unterminated long string") end
		local body = src:sub(pos + #open, q - 1)
		if body:sub(1, 1) == "\n" then body = body:sub(2) end
		local t = tok("str", body)
		line = line + count_newlines(body)
		pos = q + #close
		return t
	end

	local function try_long_string_open()
		-- returns number of '=' if a long string opens here, else nil
		if src:sub(pos, pos) ~= "[" then return nil end
		local p = pos
		local lvl = 0
		while src:sub(p + 1 + lvl, p + 1 + lvl) == "=" do
			lvl = lvl + 1
			p = p + 1
		end
		if src:sub(p + 1, p + 1) == "[" then return lvl end
		return nil
	end

	local function lex_name()
		local s = src:match("^[%a_][%w_]*", pos)
		if not s then err("expected name") end
		local word = s
		adv(#word)
		if KEYWORDS[word] then
			return tok("kw", word)
		end
		return tok("name", word)
	end

	local function read_backtick_string()
		-- Luau string interpolation: `text {expr} text`
		-- pos is at the opening backtick.
		local parts = {}
		local has_expr = false
		local cur = {}
		adv(1) -- opening backtick

		local function flush_str()
			if #cur > 0 or (#parts == 0) then
				parts[#parts + 1] = { t = "str", v = table.concat(cur) }
			end
			cur = {}
		end

		while true do
			if pos > n then err("unterminated interpolated string") end
			local c = src:sub(pos, pos)
			if c == "\n" or c == "\r" then
				err("unterminated interpolated string (newline)")
			elseif c == "`" then
				adv(1)
				flush_str()
				break
			elseif c == "\\" then
				local e = src:sub(pos + 1, pos + 1)
				if e == "\n" then
					adv(1)
					line_cont()
				elseif e == "" then
					err("unterminated interpolated string escape")
				elseif e == "n" or e == "t" or e == "r" or e == "a" or e == "b"
					or e == "f" or e == "v" or e == "\\" or e == "`"
					or e == "{" or e == "}" or e == '"' or e == "'" then
					local ch
					if e == "n" then ch = "\n"
					elseif e == "t" then ch = "\t"
					elseif e == "r" then ch = "\r"
					elseif e == "a" then ch = "\a"
					elseif e == "b" then ch = "\b"
					elseif e == "f" then ch = "\f"
					elseif e == "v" then ch = "\v"
					else ch = e
					end
					adv(2)
					table.insert(cur, ch)
				elseif is_digit(e) then
					local d = src:match("^\\([%d][%d]?[%d]?)", pos)
					local num = tonumber(d)
					if num == nil or num > 255 then err("invalid escape value") end
					adv(1 + #d)
					table.insert(cur, string.char(num))
				elseif e == "x" then
					local h = src:match("^\\(%x%x?)", pos)
					adv(1 + #h)
					table.insert(cur, string.char(tonumber(h:sub(2, 2), 16)))
				else
					err("invalid escape sequence")
				end
			elseif c == "{" then
				-- placeholder: {expression}
				flush_str()
				has_expr = true
				adv(1) -- consume '{'
				-- capture balanced expression text (braces matched; strings skipped)
				local depth = 1
				local buf = {}
				while true do
					if pos > n then err("unterminated interpolated string") end
					local ch = src:sub(pos, pos)
					if ch == "\n" then err("unterminated interpolated string (newline)") end
					if ch == '"' or ch == "'" then
						-- skip quoted string (with escapes)
						local q = ch
						table.insert(buf, ch)
						adv(1)
						while pos <= n do
							local sc = src:sub(pos, pos)
							table.insert(buf, sc)
							adv(1)
							if sc == "\\" and pos <= n then
								table.insert(buf, src:sub(pos, pos))
								adv(1)
							elseif sc == q then
								break
							end
						end
					elseif ch == "[" and (src:sub(pos + 1, pos + 1) == "[" or src:sub(pos + 1, pos + 1) == "=") then
						local lvl = 0
						local p2 = pos
						while src:sub(p2 + 1 + lvl, p2 + 1 + lvl) == "=" do
							lvl = lvl + 1
							p2 = p2 + 1
						end
						if src:sub(p2 + 1, p2 + 1) == "[" then
							local close = "]" .. string.rep("=", lvl) .. "]"
							local q2 = src:find(close, pos + 2 + lvl, true)
							if not q2 then err("unterminated long string") end
							for i = pos, q2 + #close - 1 do
								table.insert(buf, src:sub(i, i))
							end
							pos = q2 + #close
						else
							table.insert(buf, ch)
							adv(1)
						end
					elseif ch == "{" then
						depth = depth + 1
						table.insert(buf, ch)
						adv(1)
					elseif ch == "}" then
						depth = depth - 1
						if depth == 0 then
							adv(1)
							break
						end
						table.insert(buf, ch)
						adv(1)
					elseif ch == "-" and src:sub(pos + 1, pos + 1) == "-" then
						-- comments inside placeholder: skip to end of line (or long comment)
						local p2 = pos + 2
						local lvl = 0
						while src:sub(p2 + lvl, p2 + lvl) == "=" do
							lvl = lvl + 1
							p2 = p2 + 1
						end
						if src:sub(p2, p2) == "[" then
							local close = "]" .. string.rep("=", lvl) .. "]"
							local q2 = src:find(close, p2 + 1, true)
							if not q2 then err("unterminated comment") end
							pos = q2 + #close
						else
							local q2 = src:find("\n", pos, true)
							if not q2 then
								pos = n + 1
							else
								pos = q2
							end
						end
					else
						table.insert(buf, ch)
						adv(1)
					end
				end
				local text = table.concat(buf)
				if text:match("^%s*{") then
					err("double braces are not permitted within interpolated strings; use '\\{' for a literal brace")
				end
				parts[#parts + 1] = { t = "expr", v = text }
			else
				table.insert(cur, c)
				adv(1)
			end
		end

		if not has_expr then
			return tok("bstr", table.concat(cur))
		end
		local t = tok("interp", parts)
		return t
	end

	while true do
		skip_ws_and_comments()
		if pos > n then
			tok("eof", nil)
			break
		end
		local c = src:sub(pos, pos)

		if is_alpha(c) then
			lex_name()
		elseif is_digit(c) or (c == "." and is_digit(src:sub(pos + 1, pos + 1))) then
			lex_number()
		elseif c == '"' or c == "'" then
			lex_short_string(c)
		elseif try_long_string_open() then
			lex_long_string()
		elseif opts.luau and c == "`" then
			read_backtick_string()
		elseif c == ":" and src:sub(pos + 1, pos + 1) == ":" then
			local name = src:match("^::([%a_][%w_]*)::", pos)
			if name then
				tok("label", name)
				adv(#name + 4)
			else
				tok("punct", ":")
				adv(1)
			end
		else
			local matched = false
			for _, two in ipairs(TWO) do
				if src:sub(pos, pos + #two - 1) == two then
					tok("punct", two)
					adv(#two)
					matched = true
					break
				end
			end
			if not matched then
				if SINGLE:find(c, 1, true) then
					tok("punct", c)
					adv(1)
				else
					err(string.format("unexpected character %q", c))
				end
			end
		end
	end

	return toks
end

Lex.new = new
Lex.KEYWORDS = KEYWORDS

return Lex
