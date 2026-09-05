//! Mod — V[a] = V[b] % V[c] with __mod fallback.

use crate::vmgen::handlers::bin_op_body;
use crate::vmgen::strpool::StrPool;

pub fn code(fmt: u8, p: &mut StrPool) -> String {
	bin_op_body(fmt, "__mod", &|x, y| format!("{x} % {y}"), p)
}
