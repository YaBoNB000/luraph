//! Sub — V[a] = V[b] - V[c] with __sub fallback.

use crate::vmgen::handlers::bin_op_body;
use crate::vmgen::strpool::StrPool;

pub fn code(fmt: u8, p: &mut StrPool) -> String {
	bin_op_body(fmt, "__sub", &|x, y| format!("{x} - {y}"), p)
}
