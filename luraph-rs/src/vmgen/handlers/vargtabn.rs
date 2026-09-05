//! VarArgTabN — append varargs into table V[a] from counter V[b].

/// f0 canonical numeric-for; f1 equivalent while form.
pub fn code(fmt: u8) -> String {
	match fmt {
		0 => "local t = V[a + 1]; local n = V[b + 1]; for i = 1, vargc do t[n + i] = vargs[i] end; V[b + 1] = n + vargc".to_string(),
		_ => "local t = V[a + 1]; local n = V[b + 1]; local i = 1; while i <= vargc do t[n + i] = vargs[i]; i = i + 1 end; V[b + 1] = n + vargc".to_string(),
	}
}
