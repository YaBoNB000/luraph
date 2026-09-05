//! SetTab — V[a][V[b]] = V[c] with full __newindex semantics.

use crate::vmgen::strpool::StrPool;

/// f0 canonical; f1 renamed temporaries (o/j/w/m0).
pub fn code(fmt: u8, p: &mut StrPool) -> String {
	let table = p.lit("table");
	let newindex = p.lit("__newindex");
	let function = p.lit("function");
	let msg = p.lit("attempt to index a ");
	let value = p.lit(" value");
	match fmt {
		0 => format!(
			"local t = V[a + 1]; local k = V[b + 1]; local v = V[c + 1]; if TYP(t) ~= {table} then local mt = GMT(t); local f = mt and mt[{newindex}]; if TYP(f) == {function} then f(t, k, v) elseif f ~= nil then RSET(f, k, v) else ERR({msg} .. TYP(t) .. {value}, 0) end else if RGET(t, k) == nil then local f = mget(t, {newindex}); if TYP(f) == {function} then f(t, k, v) elseif f ~= nil then RSET(f, k, v) else RSET(t, k, v) end else RSET(t, k, v) end end"
		),
		_ => format!(
			"local o = V[a + 1]; local j = V[b + 1]; local w = V[c + 1]; if TYP(o) ~= {table} then local m0 = GMT(o); local f = m0 and m0[{newindex}]; if TYP(f) == {function} then f(o, j, w) elseif f ~= nil then RSET(f, j, w) else ERR({msg} .. TYP(o) .. {value}, 0) end else if RGET(o, j) == nil then local f = mget(o, {newindex}); if TYP(f) == {function} then f(o, j, w) elseif f ~= nil then RSET(f, j, w) else RSET(o, j, w) end else RSET(o, j, w) end end"
		),
	}
}
