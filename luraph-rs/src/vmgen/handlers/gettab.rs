//! GetTab — V[a] = V[b][V[c]] with full __index semantics.

use crate::vmgen::strpool::StrPool;

/// f0 canonical; f1 renamed temporaries (o/j/m0).
pub fn code(fmt: u8, p: &mut StrPool) -> String {
	let table = p.lit("table");
	let index = p.lit("__index");
	let function = p.lit("function");
	let msg = p.lit("attempt to index a ");
	let value = p.lit(" value");
	match fmt {
		0 => format!(
			"local t = V[b + 1]; local k = V[c + 1]; local r; if TYP(t) == {table} then r = RGET(t, k); if r == nil then local f = mget(t, {index}); if TYP(f) == {function} then r = f(t, k) elseif f ~= nil then r = f[k] end end else local mt = GMT(t); if mt and mt[{index}] ~= nil then local f = mt[{index}]; if TYP(f) == {function} then r = f(t, k) else r = f[k] end else ERR({msg} .. TYP(t) .. {value}, 0) end end; V[a + 1] = r"
		),
		_ => format!(
			"local o = V[b + 1]; local j = V[c + 1]; local r; if TYP(o) == {table} then r = RGET(o, j); if r == nil then local f = mget(o, {index}); if TYP(f) == {function} then r = f(o, j) elseif f ~= nil then r = f[j] end end else local m0 = GMT(o); if m0 and m0[{index}] ~= nil then local f = m0[{index}]; if TYP(f) == {function} then r = f(o, j) else r = f[j] end else ERR({msg} .. TYP(o) .. {value}, 0) end end; V[a + 1] = r"
		),
	}
}
