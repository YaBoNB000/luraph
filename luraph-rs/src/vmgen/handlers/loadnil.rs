//! LoadNil — nil a contiguous register range (walks b slots from a).

/// f0 canonical numeric-for; f1 equivalent while form.
pub fn code(fmt: u8) -> String {
	match fmt {
		0 => "for i = 1, b do V[a + i] = nil end".to_string(),
		_ => "local i = 1; while i <= b do V[a + i] = nil; i = i + 1 end".to_string(),
	}
}

/// v15 stage A: range steps translate through the scattered slot table.
pub fn code_v15(fmt: u8) -> String {
	match fmt {
		0 => "for i = 1, b do V[S[a + i]] = nil end".to_string(),
		_ => "local i = 1; while i <= b do V[S[a + i]] = nil; i = i + 1 end".to_string(),
	}
}
