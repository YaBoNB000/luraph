//! Jmp — unconditional jump (pc = target).

/// f0 canonical; f1 do-block wrap.
pub fn code(fmt: u8) -> String {
	match fmt {
		0 => "pc = b".to_string(),
		_ => "do pc = b end".to_string(),
	}
}
