//! Eq — V[a] = (V[b] == V[c]) with __eq fallback.

use crate::vmgen::strpool::StrPool;

/// f0 canonical; f1 renamed temporaries (u/w/tu/tw).
pub fn code(fmt: u8, p: &mut StrPool) -> String {
	let number = p.lit("number");
	let string = p.lit("string");
	let boolean = p.lit("boolean");
	let nil = p.lit("nil");
	let eq = p.lit("__eq");
	match fmt {
		0 => format!(
			"local x = V[b + 1]; local y = V[c + 1]; local tx = TYP(x); local ty = TYP(y); if tx == ty and (tx == {number} or tx == {string} or tx == {boolean} or tx == {nil}) then V[a + 1] = x == y else local f = mget(x, {eq}) or mget(y, {eq}); if f then V[a + 1] = f(x, y) else V[a + 1] = x == y end end"
		),
		_ => format!(
			"local u = V[b + 1]; local w = V[c + 1]; local tu = TYP(u); local tw = TYP(w); if tu == tw and (tu == {number} or tu == {string} or tu == {boolean} or tu == {nil}) then V[a + 1] = u == w else local f = mget(u, {eq}) or mget(w, {eq}); if f then V[a + 1] = f(u, w) else V[a + 1] = u == w end end"
		),
	}
}
