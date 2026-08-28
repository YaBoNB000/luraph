//! L1 name mangling: rename every local symbol (params, locals, loop vars,
//! local function names) to a random identifier.
//!
//! Collision safety: new names avoid Lua keywords, Luau contextual keywords,
//! every global name referenced by the program (avoid shadowing), and each
//! other. Globals themselves are NOT renamed (that would require environment
//! wrapping — out of scope for v1).

use crate::rng::Rng;
use crate::symtab::SymTable;
use std::collections::HashSet;

/// 5.1 keywords + Luau contextual keywords (never usable as our random names)
pub const RESERVED: &[&str] = &[
	"and", "break", "do", "else", "elseif", "end", "false", "for", "function", "if", "in",
	"local", "nil", "not", "or", "repeat", "return", "then", "true", "until", "while",
	"continue", "goto",
	// "self" is the FIXED name of the implicit method parameter (keep_name)
	// — a mangled name must never collide with it (same-scope shadowing)
	"self",
];

pub fn reserved_set(extra: &HashSet<String>) -> HashSet<String> {
	let mut s: HashSet<String> = RESERVED.iter().map(|x| x.to_string()).collect();
	s.extend(extra.iter().cloned());
	s
}

/// Generate a random identifier not in `reserved`.
/// Style mix: ~30% short (2-4), ~40% medium (5-8), ~30% long (9-15).
pub fn gen_name(rng: &mut Rng, reserved: &HashSet<String>) -> String {
	loop {
		let short = rng.int(0, 99);
		let len = if short < 30 {
			rng.int(2, 4)
		} else if short < 70 {
			rng.int(5, 8)
		} else {
			rng.int(9, 15)
		};
		let first = if rng.int(0, 1) == 0 {
			rng.int(b'a' as i64, b'z' as i64)
		} else {
			rng.int(b'A' as i64, b'Z' as i64)
		};
		let mut s = String::new();
		s.push(first as u8 as char);
		for _ in 1..len {
			let r = rng.int(0, 99);
			let c = if r < 55 {
				rng.int(b'a' as i64, b'z' as i64) as u8 as char
			} else if r < 85 {
				rng.int(b'0' as i64, b'9' as i64) as u8 as char
			} else if r < 95 {
				rng.int(b'A' as i64, b'Z' as i64) as u8 as char
			} else {
				'_'
			};
			s.push(c);
		}
		if !reserved.contains(&s) {
			return s;
		}
	}
}

/// v15 naming scheme (sample style): 1-2 char names from the 53-base
/// alphabet (A-Z / `_` / a-z). Used for the v15 profile so mangled
/// locals match the sample's name lengths (fingerprints F14/F21).
fn gen_name_v15(rng: &mut Rng, reserved: &HashSet<String>) -> String {
	const ALPHA: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ_abcdefghijklmnopqrstuvwxyz";
	loop {
		// sample names are overwhelmingly 1 char; keep 80/20
		let len = if rng.int(0, 99) < 80 { 1 } else { 2 };
		let mut s = String::new();
		for _ in 0..len {
			s.push(ALPHA[rng.int(0, 52) as usize] as char);
		}
		if !reserved.contains(&s) {
			return s;
		}
	}
}

pub fn mangle(table: &mut SymTable, rng: &mut Rng, v15: bool) {
	let mut reserved =
		reserved_set(&table.globals.iter().cloned().collect::<HashSet<_>>());
	let n = table.syms.len();
	let mut names: Vec<Option<String>> = Vec::with_capacity(n);
	for sym in table.syms.iter() {
		if sym.keep_name {
			names.push(None); // fixed name (implicit self) — do not rename
		} else if v15 {
			let name = gen_name_v15(rng, &reserved);
			reserved.insert(name.clone());
			names.push(Some(name));
		} else {
			let name = gen_name(rng, &reserved);
			reserved.insert(name.clone());
			names.push(Some(name));
		}
	}
	for (i, name) in names.into_iter().enumerate() {
		if let Some(name) = name {
			table.syms[i].name = name;
		}
	}
}
