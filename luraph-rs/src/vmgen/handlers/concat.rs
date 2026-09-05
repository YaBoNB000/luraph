//! Concat — V[a] = V[b] .. V[c] with __concat fallback.

use crate::vmgen::strpool::StrPool;

/// f0 canonical; f1 renamed temporaries (u/w/tu/tw).
pub fn code(fmt: u8, p: &mut StrPool) -> String {
	let number = p.lit("number");
	let string = p.lit("string");
	let concat = p.lit("__concat");
	let msg = p.lit("attempt to perform concatenation on a ");
	let value = p.lit(" value");
	match fmt {
		0 => format!(
			"local x = V[b + 1]; local y = V[c + 1]; local tx = TYP(x); local ty = TYP(y); if (tx == {number} or tx == {string}) and (ty == {number} or ty == {string}) then V[a + 1] = x .. y else local f = mget(x, {concat}) or mget(y, {concat}); if f then V[a + 1] = f(x, y) else ERR({msg} .. tx .. {value}, 0) end end"
		),
		_ => format!(
			"local u = V[b + 1]; local w = V[c + 1]; local tu = TYP(u); local tw = TYP(w); if (tu == {number} or tu == {string}) and (tw == {number} or tw == {string}) then V[a + 1] = u .. w else local f = mget(u, {concat}) or mget(w, {concat}); if f then V[a + 1] = f(u, w) else ERR({msg} .. tu .. {value}, 0) end end"
		),
	}
}
