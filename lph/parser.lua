-- lph/parser.lua
-- Recursive-descent parser for Lua 5.1 and Luau source, producing an AST.
-- This file must stay valid plain Lua 5.1.
--
-- AST node kinds
--   statements: Chunk Block Local LocalFunc FuncDecl If While Repeat ForNum
--               ForGen Do Break Continue Return ExprStat Assign GotoStat LabelStat
--   expressions: Num Str Bool Nil Vararg Ident Dot Index Call Un Bin Table
--                Function Interp
--
-- Luau extras (dialect = "luau"): continue, compound assignment (a += b),
-- floor division (//), string interpolation (`a {e} b`), type annotations
-- (parsed and dropped), type aliases (type X = ... parsed and dropped).

local Lex = require("lph.lexer")

local Parser = {}

local BLOCK_END = { end = true, else = true, elseif = true, until = true }
local CMP = { ["<"] = true, [">"] = true, ["<="] = true, [">="] = true, ["=="] = true, ["~="] = true }

-- Pratt priorities (identical to Lua 5.1 lparser.c; // added at mul level for Luau)
local PRI = {
	["+"] = { 6, 6 }, ["-"] = { 6, 6 }, ["*"] = { 6, 6 }, ["/"] = { 6, 6 }, ["%"] = { 6, 6 },
	["//"] = { 6, 6 },
	["^"] = { 10, 9 }, -- right associative
	[".."] = { 5, 4 }, -- right associative
	["=="] = { 3, 3 }, ["~="] = { 3, 3 }, ["<"] = { 3, 3 }, [">"] = { 3, 3 },
	["<="] = { 3, 3 }, [">="] = { 3, 3 },
	["and"] = { 2, 2 }, ["or"] = { 1, 1 },
}
local UNARY_PRI = 8
local COMPOUND = { ["+="] = "+", ["-="] = "-", ["*="] = "*", ["/="] = "/", ["//="] = "//", ["%="] = "%", ["^="] = "^" }

local function tokdesc(t)
	if not t then return "end of file" end
	if t.k == "name" or t.k == "kw" or t.k == "label" then return "'" .. tostring(t.v) .. "'" end
	if t.k == "num" then return "number " .. tostring(t.v) end
	if t.k == "str" or t.k == "bstr" or t.k == "interp" then return "string" end
	if t.k == "eof" then return "end of file" end
	return "'" .. tostring(t.v) .. "'"
end

local function new(toks, opts)
	local p = { toks = toks, i = 1, luau = (opts and opts.luau) or false }

	function p:peek(ahead)
		ahead = ahead or 0
		return self.toks[self.i + ahead - 1]
	end

	function p:next()
		local t = self.toks[self.i]
		self.i = self.i + 1
		return t
	end

	function p:is(kind, value)
		local t = self:peek(0)
		if not t then return false end
		return t.k == kind and (value == nil or t.v == value)
	end

	function p:eat(kind, value)
		if self:is(kind, value) then
			return self:next()
		end
		return nil
	end

	function p:expect(kind, value)
		local t = self:peek(0)
		if t and t.k == kind and (value == nil or t.v == value) then
			self.i = self.i + 1
			return t
		end
		local want = kind
		if value then want = want .. " '" .. value .. "'" end
		error(string.format("parse error line %s: expected %s, got %s",
			(t and t.line) or "?", want, tokdesc(t)), 0)
	end

	function p:errf(fmt, ...)
		local t = self:peek(0)
		error(string.format("parse error line %s: " .. fmt, (t and t.line) or "?", ...), 0)
	end

	------------------------------------------------------------------
	-- statements

	function p:chunk()
		local block = self:block()
		return { kind = "Chunk", body = block }
	end

	function p:block()
		local stmts = {}
		while true do
			local t = self:peek(0)
			if not t or t.k == "eof" then break end
			if t.k == "kw" and BLOCK_END[t.v] then break end
			if t.k == "kw" and t.v == "return" then
				stmts[#stmts + 1] = self:returnstat()
				self:eat("punct", ";")
				break
			end
			stmts[#stmts + 1] = self:stat()
			self:eat("punct", ";")
		end
		return { kind = "Block", stmts = stmts }
	end

	function p:stat()
		local t = self:peek(0)
		local v = (t.k == "kw") and t.v or nil

		if t.k == "label" then
			self:next()
			self:errf("labels (::%s::) are not supported", t.v)
		end

		if v == "local" then return self:localstat() end
		if v == "function" then return self:functionstat() end
		if v == "if" then return self:ifstat() end
		if v == "while" then return self:whilestat() end
		if v == "do" then
			self:next()
			local b = self:block()
			self:expect("kw", "end")
			return { kind = "Do", body = b }
		end
		if v == "repeat" then return self:repeatstat() end
		if v == "for" then return self:forstat() end
		if v == "break" then
			self:next()
			return { kind = "Break" }
		end
		if v == "return" then
			return self:returnstat()
		end

		-- expression statement; also Luau contextual keywords
		if t.k == "name" then
			if t.v == "continue" and not self:is("punct", "=") and not self:is("punct", ",")
				and not self:is("punct", "(") and not self:is("punct", ".") and not self:is("punct", ":") then
				if not self.luau then
					self:errf("'continue' requires the Luau dialect (use --dialect luau)")
				end
				self:next()
				return { kind = "Continue" }
			end
			if t.v == "type" and self:peek(1) and self:peek(1).k == "name" then
				return self:typealiasstat()
			end
			if t.v == "export" and self:peek(1) and self:peek(1).k == "name"
				and self:peek(1).v == "type" and self:peek(2) and self:peek(2).k == "name" then
				self:next()
				self:next()
				return self:typealiasstat_body()
			end
			if t.v == "goto" then
				self:errf("'goto' is not supported (not part of Lua 5.1 / stable Luau)")
			end
		end

		return self:exprstat()
	end

	function p:returnstat()
		self:expect("kw", "return")
		local exprs = {}
		if not self:is("punct", ";") and not self:is("kw", "end") and not self:is("kw", "else")
			and not self:is("kw", "elseif") and not self:is("kw", "until") and not self:is("eof") then
			exprs = self:explist()
		end
		return { kind = "Return", exprs = exprs }
	end

	function p:localstat()
		self:expect("kw", "local")
		if self:is("kw", "function") then
			self:next()
			local nm = self:expect("name")
			local body = self:funcbody()
			return { kind = "LocalFunc", name = nm.v, params = body.params, vararg = body.vararg, body = body.body }
		end
		local names = {}
		local values = {}
		local t = self:expect("name")
		names[#names + 1] = t.v
		self:eat("punct", ":") -- optional annotation: name: Type
		if self:eat("punct", ":") then
			self:typeref()
		end
		local hasval = self:eat("punct", "=")
		if hasval then
			values[1] = self:exp()
		else
			values[1] = nil
		end
		while self:eat("punct", ",") do
			local t2 = self:expect("name")
			names[#names + 1] = t2.v
			self:eat("punct", "?") -- optional marker
			if self:eat("punct", ":") then
				self:typeref()
			end
			if self:is("punct", "=") then
				self:next()
				values[#names] = self:exp()
			else
				values[#names] = nil
				if hasval == false and #values > 1 then
					-- fine: local a, b (no values)
				end
			end
		end
		-- if first had no '=', none may have '='
		for i = 2, #names do
			if values[i] ~= nil and values[1] == nil then
				self:errf("mixed local declaration with and without values")
			end
		end
		return { kind = "Local", names = names, values = values }
	end

	function p:functionstat()
		self:expect("kw", "function")
		return self:funcdecl()
	end

	function p:funcdecl()
		local t = self:expect("name")
		local parts = { t.v }
		local ismethod = false
		while true do
			if self:eat("punct", ".") then
				local nt = self:expect("name")
				parts[#parts + 1] = nt.v
			elseif self:eat("punct", ":") then
				local nt = self:expect("name")
				parts[#parts + 1] = nt.v
				ismethod = true
				break
			else
				break
			end
		end
		local body = self:funcbody()
		local node = { kind = "FuncDecl", parts = parts, ismethod = ismethod, params = body.params, vararg = body.vararg, body = body.body }
		return node
	end

	-- funcbody: '(' params ')' [':' typeref] block 'end'
	-- `prefix` names an implicit self (method declaration); pass true.
	function p:funcbody(withself)
		self:expect("punct", "(")
		local params = {}
		if withself then params[#params + 1] = "self" end
		while not self:is("punct", ")") do
			if self:is("punct", "...") then
				self:next()
			elseif self:is("name") then
				local t = self:next()
				params[#params + 1] = t.v
				self:eat("punct", "?")
				if self:eat("punct", ":") then
					self:typeref()
				end
			elseif self:is("kw", "self") then
				-- 'self' is not a reserved word; it is lexed as a name. Keep branch for clarity.
				self:next()
				params[#params + 1] = "self"
			else
				self:errf("invalid parameter")
			end
			if not self:is("punct", ")") then
				self:expect("punct", ",")
			end
		end
		self:expect("punct", ")")
		if self:eat("punct", ":") then
			self:typeref() -- return type
		end
		local block = self:block()
		self:expect("kw", "end")
		return { params = params, vararg = self:had_vararg(params), body = block }
	end

	-- vararg detection: we tracked '...' by appending it as a param marker
	function p:had_vararg(params)
		return false
	end

	function p:ifstat()
		self:expect("kw", "if")
		local cond = self:exp()
		self:expect("kw", "then")
		local thenb = self:block()
		local elsifs = {}
		while self:is("kw", "elseif") do
			self:next()
			local c2 = self:exp()
			self:expect("kw", "then")
			elsifs[#elsifs + 1] = { cond = c2, body = self:block() }
		end
		local elseb = nil
		if self:eat("kw", "else") then
			elseb = self:block()
		end
		self:expect("kw", "end")
		return { kind = "If", cond = cond, thenb = thenb, elsifs = elsifs, elseb = elseb }
	end

	function p:whilestat()
		self:expect("kw", "while")
		local cond = self:exp()
		self:expect("kw", "do")
		local body = self:block()
		self:expect("kw", "end")
		return { kind = "While", cond = cond, body = body }
	end

	function p:repeatstat()
		self:expect("kw", "repeat")
		local body = self:block()
		self:expect("kw", "until")
		local cond = self:exp()
		return { kind = "Repeat", body = body, cond = cond }
	end

	function p:forstat()
		self:expect("kw", "for")
		local var = self:expect("name")
		if self:eat("punct", "=") then
			local start = self:exp()
			self:expect("punct", ",")
			local limit = self:exp()
			local step = nil
			if self:is("punct", "+") or self:is("punct", "-") or self:is("num") or self:is("punct", "(")
				or self:is("kw") or self:is("name") or self:is("str") then
				-- only if next token can start an expression (i.e. not 'do')
				if not self:is("kw", "do") then
					step = self:exp()
				end
			end
			self:expect("kw", "do")
			local body = self:block()
			self:expect("kw", "end")
			return { kind = "ForNum", var = var.v, start = start, limit = limit, step = step, body = body }
		else
			local vars = { var.v }
			while self:eat("punct", ",") do
				local nv = self:expect("name")
				vars[#vars + 1] = nv.v
			end
			self:expect("kw", "in")
			local iters = self:explist()
			self:expect("kw", "do")
			local body = self:block()
			self:expect("kw", "end")
			return { kind = "ForGen", vars = vars, iters = iters, body = body }
		end
	end

	-- type X = ...   (Luau type alias: parsed and dropped)
	function p:typealiasstat()
		self:expect("name") -- 'type'
		return self:typealiasstat_body()
	end

	function p:typealiasstat_body()
		self:expect("name") -- alias name
		if self:is("punct", "<") then
			-- generic parameters: <T, U, ...>  (names only, may carry constraints we ignore loosely)
			local depth = 1
			self:next()
			while depth > 0 do
				local t = self:peek(0)
				if t.k == "eof" then self:errf("unterminated generic parameters") end
				if t.v == "<" then depth = depth + 1
				elseif t.v == ">" then depth = depth - 1 end
				self:next()
			end
		end
		self:expect("punct", "=")
		self:typeref()
		return { kind = "TypeAlias" }
	end

	------------------------------------------------------------------
	-- type annotations (parsed and dropped)

	function p:typeref()
		return p_union(self)
	end

	local function p_union(p)
		local t = p_inter(p)
		while p:is("punct", "|") do
			p:next()
			p_inter(p)
		end
		return t
	end

	local function p_inter(p)
		local t = p_primary_type(p)
		while p:is("punct", "&") do
			p:next()
			p_primary_type(p)
		end
		return t
	end

	local function p_primary_type(p)
		local tk = p:peek(0)
		if not tk then p:errf("expected type") end
		if tk.k == "name" then
			p:next()
			-- optional dot chain
			while p:is("punct", ".") do
				p:next()
				if p:peek(0).k ~= "name" then p:errf("expected name in type") end
				p:next()
			end
			-- optional generic args
			if p:is("punct", "<") then
				local depth = 1
				p:next()
				while depth > 0 do
					local t = p:peek(0)
					if t.k == "eof" then p:errf("unterminated generic type") end
					if t.v == "<" then depth = depth + 1
					elseif t.v == ">" then depth = depth - 1 end
					p:next()
				end
			end
			-- function type: name '(' ... ')' ':' type   (e.g. typeof(x) is not supported)
			if p:is("punct", "(") then
				p:errf("unsupported type syntax (function/typeof types not fully supported); simplify the annotation")
			end
			return t
		elseif tk.k == "punct" and tk.v == "(" then
			-- function type or parenthesized type
			p:next()
			if not p:is("punct", ")") then
				while true do
					p_union(p)
					if p:eat("punct", ",") then
						-- allow trailing comma
					else
						break
					end
				end
			end
			p:expect("punct", ")")
			if p:eat("punct", ":") then
				p_union(p)
			end
			return t
		elseif tk.k == "punct" and tk.v == "{" then
			-- table type
			p:next()
			while not p:is("punct", "}") do
				if p:is("name") or p:is("str") or p:is("punct", "[") then
					if p:is("punct", "[") then
						p:next()
						p:expect("punct", "]")
					elseif p:is("str") then
						p:next()
					else
						p:next()
					end
					p:expect("punct", ":")
					p_union(p)
				elseif p:is("punct", "...") then
					p:next()
				else
					p:errf("unsupported table type syntax")
				end
				if not p:is("punct", "}") then
					p:eat("punct", ",")
				end
			end
			p:expect("punct", "}")
			return t
		elseif tk.k == "str" then
			p:next()
			return t
		elseif tk.k == "punct" and tk.v == "..." then
			p:next()
			return t
		elseif tk.k == "punct" and tk.v == "?" then
			-- optional type (rare in our positions)
			p:next()
			p_union(p)
			return t
		else
			p:errf("unsupported or missing type annotation")
		end
	end

	------------------------------------------------------------------
	-- expression statements and assignments

	function p:exprstat()
		local e = self:prefixexp()
		local t = self:peek(0)
		if t and t.k == "punct" and t.v == "=" then
			local targets = { e }
			while self:eat("punct", ",") do
				targets[#targets + 1] = self:assign_target()
			end
			self:next() -- '='
			local values = self:explist()
			return { kind = "Assign", targets = targets, values = values }
		elseif t and t.k == "punct" and COMPOUND[t.v] then
			if not self.luau then
				self:errf("compound assignment (%s) requires the Luau dialect", t.v)
			end
			local op = self:next().v
			local value = self:exp()
			local lhs = e
			local rhs = clone(lhs)
			local bin = { kind = "Bin", op = COMPOUND[op], l = rhs, r = value }
			return { kind = "Assign", targets = { lhs }, values = { bin } }
		end
		if e.kind ~= "Call" then
			self:errf("expected assignment or a function call")
		end
		return { kind = "ExprStat", expr = e }
	end

	function p:assign_target()
		local e = self:simple_assign_target()
		-- allow suffix (index / dot) chains
		while true do
			if self:is("punct", ".") then
				self:next()
				local nm = self:expect("name")
				e = { kind = "Dot", obj = e, name = nm.v }
			elseif self:is("punct", "[") then
				self:next()
				local idx = self:exp()
				self:expect("punct", "]")
				e = { kind = "Index", obj = e, idx = idx }
			else
				break
			end
		end
		return e
	end

	function p:simple_assign_target()
		local t = self:peek(0)
		if t.k == "name" then
			self:next()
			return { kind = "Ident", name = t.v }
		end
		-- '(' exp ')' is not a valid assignment target
		self:errf("invalid assignment target")
	end

	function p:explist()
		local list = { self:exp() }
		while self:eat("punct", ",") do
			list[#list + 1] = self:exp()
		end
		return list
	end

	------------------------------------------------------------------
	-- expressions (Pratt, matches Lua 5.1 lparser.c exactly)

	function p:exp()
		return self:subexpr(0)
	end

	function p:subexpr(limit)
		local t = self:peek(0)
		if t.k == "punct" and (t.v == "-" or t.v == "#") then
			self:next()
			local e = self:subexpr(UNARY_PRI)
			local node = { kind = "Un", op = t.v, e = e }
			return self:binloop(node, limit)
		elseif t.k == "kw" and t.v == "not" then
			self:next()
			local e = self:subexpr(UNARY_PRI)
			local node = { kind = "Un", op = "not", e = e }
			return self:binloop(node, limit)
		end
		local e = self:simpleexp()
		return self:binloop(e, limit)
	end

	function p:binloop(e, limit)
		while true do
			local t = self:peek(0)
			local op = nil
			if t then
				if t.k == "punct" and PRI[t.v] then
					op = t.v
				elseif t.k == "kw" and (t.v == "and" or t.v == "or") then
					op = t.v
				end
			end
			if not op then break end
			local pr = PRI[op]
			if pr[1] <= limit then break end
			self:next()
			local r = self:subexpr(pr[2])
			e = { kind = "Bin", op = op, l = e, r = r }
		end
		return e
	end

	function p:simpleexp()
		local t = self:peek(0)
		if t.k == "num" then
			self:next()
			return { kind = "Num", value = t.v, isfloat = t.isfloat or false }
		elseif t.k == "str" or t.k == "bstr" then
			self:next()
			return { kind = "Str", value = t.v }
		elseif t.k == "interp" then
			return self:interpexpr()
		elseif t.k == "kw" and t.v == "true" then
			self:next()
			return { kind = "Bool", value = true }
		elseif t.k == "kw" and t.v == "false" then
			self:next()
			return { kind = "Bool", value = false }
		elseif t.k == "kw" and t.v == "nil" then
			self:next()
			return { kind = "Nil" }
		elseif t.k == "punct" and t.v == "..." then
			self:next()
			return { kind = "Vararg" }
		elseif t.k == "punct" and t.v == "{" then
			return self:tableconstructor()
		elseif t.k == "kw" and t.v == "function" then
			self:next()
			local body = self:funcbody()
			return { kind = "Function", params = body.params, vararg = body.vararg, body = body.body }
		end
		return self:prefixexp()
	end

	function p:interpexpr()
		local t = self:next()
		local parts = {}
		for _, part in ipairs(t.v) do
			if part.t == "str" then
				parts[#parts + 1] = { t = "str", v = part.v }
			else
				local subp = Parser.new_parser(part.v, { luau = self.luau })
				local e = subp:exp()
				if not subp:is("eof") then
					self:errf("malformed interpolated string expression")
				end
				parts[#parts + 1] = { t = "expr", e = e }
			end
		end
		return { kind = "Interp", parts = parts }
	end

	function p:prefixexp()
		local e = self:primary()
		while true do
			if self:is("punct", ".") then
				self:next()
				local nm = self:expect("name")
				e = { kind = "Dot", obj = e, name = nm.v }
			elseif self:is("punct", ":") then
				self:next()
				local nm = self:expect("name")
				local args, ok = self:callargs()
				if not ok then
					self:errf("expected arguments for method call")
				end
				local obj = e
				local call = {
					kind = "Call",
					func = { kind = "Dot", obj = obj, name = nm.v },
					args = args,
					method = true,
					mname = nm.v,
					obj = obj,
				}
				-- args already has self prepended by callargs? No: prepend here
				table.insert(call.args, 1, clone(obj))
				e = call
			elseif self:is("punct", "(") or self:is("punct", "{") or self:is("str") or self:is("bstr") or self:is("interp") then
				local args = self:callargs()
				e = { kind = "Call", func = e, args = args }
			else
				break
			end
		end
		return e
	end

	-- callargs: '(' explist ')' | table | string
	function p:callargs()
		if self:is("punct", "(") then
			self:next()
			local args = {}
			if not self:is("punct", ")") then
				args = self:explist()
			end
			self:expect("punct", ")")
			return args, true
		elseif self:is("punct", "{") then
			return { self:tableconstructor() }, true
		elseif self:is("str") or self:is("bstr") or self:is("interp") then
			return { self:simpleexp() }, true
		end
		return nil, false
	end

	function p:primary()
		local t = self:peek(0)
		if t.k == "name" then
			self:next()
			return { kind = "Ident", name = t.v }
		elseif t.k == "punct" and t.v == "(" then
			self:next()
			local e = self:exp()
			self:expect("punct", ")")
			return e
		elseif t.k == "punct" and t.v == "{" then
			return self:tableconstructor()
		elseif t.k == "kw" and t.v == "function" then
			self:next()
			local body = self:funcbody()
			return { kind = "Function", params = body.params, vararg = body.vararg, body = body.body }
		elseif t.k == "str" or t.k == "bstr" or t.k == "interp" or t.k == "num"
			or (t.k == "kw" and (t.v == "true" or t.v == "false" or t.v == "nil"))
			or (t.k == "punct" and t.v == "...") then
			return self:simpleexp()
		end
		self:errf("unexpected token %s", tokdesc(t))
	end

	function p:tableconstructor()
		self:expect("punct", "{")
		local fields = {}
		while not self:is("punct", "}") do
			local t = self:peek(0)
			if t.k == "punct" and t.v == "[" then
				self:next()
				local k = self:exp()
				self:expect("punct", "]")
				self:expect("punct", "=")
				local v = self:exp()
				fields[#fields + 1] = { k = k, v = v }
			elseif t.k == "name" and self:peek(1) and self:peek(1).k == "punct" and self:peek(1).v == "=" then
				local nm = self:next()
				self:next() -- '='
				local v = self:exp()
				fields[#fields + 1] = { k = { kind = "Str", value = nm.v }, v = v }
			else
				local v = self:exp()
				fields[#fields + 1] = { v = v }
			end
			if not self:is("punct", "}") then
				if not (self:eat("punct", ",") or self:eat("punct", ";")) then
					self:errf("expected ',' or '}' in table constructor")
				end
			end
		end
		self:expect("punct", "}")
		return { kind = "Table", fields = fields }
	end

	return p
end

local function clone(node)
	if type(node) ~= "table" then return node end
	local out = {}
	for k, v in pairs(node) do
		out[k] = clone(v)
	end
	return out
end

function Parser.parse(src, opts)
	opts = opts or {}
	local toks
	local ok, res = pcall(Lex.new, src, opts)
	if not ok then error(res, 0) end
	toks = res
	local p = new(toks, opts)
	local chunk
	ok, chunk = pcall(p.block, p)
	if not ok then error(chunk, 0) end
	-- wrap
	local out = { kind = "Chunk", body = chunk }
	if not p:is("eof") then
		local t = p:peek(0)
		error(string.format("parse error line %s: unexpected %s", t.line, tokdesc(t)), 0)
	end
	return out
end

-- parse a bare expression (used for interpolation placeholders)
function Parser.new_parser(src, opts)
	local toks = Lex.new(src, opts)
	return new(toks, opts)
end

return Parser
