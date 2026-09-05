//! L6 VM — interpreter string pool.
//!
//! v15 stage E5 (F10): string pool. Legacy profiles inline quoted
//! literals; v15 routes every meta/type/error/select literal through
//! the boot-time MS table built from numeric char codes, so the
//! visible output keeps almost no short string literals.
//!
//! Shared by the template assembler and the per-instruction handler
//! files (建议1: one file per opcode, each returning fixed interpreter
//! code) so every handler resolves its literals through one pool.

use std::collections::HashMap;

pub struct StrPool {
	use_ms: bool,
	entries: Vec<String>,
	index: HashMap<String, usize>,
}

impl StrPool {
	pub fn new(use_ms: bool) -> StrPool {
		StrPool { use_ms, entries: Vec::new(), index: HashMap::new() }
	}
	pub fn lit(&mut self, s: &str) -> String {
		if !self.use_ms {
			return format!("'{s}'");
		}
		let n = self.entries.len();
		let e = *self.index.entry(s.to_string()).or_insert(n);
		if e == n {
			self.entries.push(s.to_string());
		}
		format!("MS[{e}]", e = e + 1)
	}
	/// Boot table emission: `MS[k] = CHAR(...)` from numeric codes.
	pub fn boot_block(&self) -> String {
		let mut s = String::from("local MS = {}\n");
		for (i, e) in self.entries.iter().enumerate() {
			let codes: Vec<String> = e.bytes().map(|b| b.to_string()).collect();
			s.push_str(&format!("  MS[{}] = CHAR({})\n", i + 1, codes.join(", ")));
		}
		s
	}
}
