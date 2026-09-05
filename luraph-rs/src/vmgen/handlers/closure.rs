//! Closure — V[a] = makefn(b): create a child-function closure.

/// f0 canonical; f1 closure through a local.
pub fn code(fmt: u8) -> String {
	match fmt {
		0 => "V[a + 1] = makefn(b + 1, V, ups)".to_string(),
		_ => "local cf = makefn(b + 1, V, ups); V[a + 1] = cf".to_string(),
	}
}

/// v15 stage A: the child resolves upvalue descriptors against the
/// parent's scattered slot table S.
pub fn code_v15(fmt: u8) -> String {
	match fmt {
		0 => "V[a + 1] = makefn(b + 1, V, ups, S)".to_string(),
		_ => "local cf = makefn(b + 1, V, ups, S); V[a + 1] = cf".to_string(),
	}
}
