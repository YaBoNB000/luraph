//! L1 minify / whitespace compression (output layer).
//!
//! Token-aware single-line compression: the (printer's) output is
//! re-lexed with the target dialect's lexer and the tokens are re-emitted
//! with the minimal whitespace Lua's syntax requires.
//!
//! Whitespace is emitted between two adjacent tokens iff BOTH boundary
//! characters are identifier characters ([0-9A-Za-z_]) — otherwise the two
//! tokens would merge into one different, longer token, e.g.
//!
//!   `local x`  ->  `localx`      (one identifier!)
//!   `a and b`  ->  `aandb`
//!   `else if`  ->  `elseif`
//!   `not not`  ->  `notnot`
//!   `end do`   ->  `enddo`
//!   `1 end`    ->  `1end` (lexes fine, but the rule is uniform & safe)
//!
//! Everything else is safe to glue: `)` + `end`, `"` + `end`, `..` + `x`,
//! `+` + `-`, `#` + `t`, `:` + `name`, ... Newlines/indentation are not
//! syntactically required in Lua (the only construct that depends on a
//! line break is a short comment, and comments are stripped by the lexer —
//! standard minify behavior). Result: one line, minimal bytes.
//!
//! Strings are re-encoded from their decoded bytes with the printer's
//! byte-exact `print_string_bytes` (escape rules + digit-merge guard), so
//! string content round-trips exactly.

use crate::lexer::{Lexer, TokKind, Token};

/// Minify Lua source: returns a single-line, whitespace-minimal version
/// with identical token sequence (hence identical semantics).
pub fn minify(src: &str, luau: bool) -> Result<String, String> {
	let mut lex = Lexer::new(src, luau);
	let toks = lex.tokens().map_err(|e| e.to_string())?;
	let mut out = String::with_capacity(src.len() / 2 + 16);
	let mut prev_end: Option<u8> = None;
	let mut prev_was_concat: bool = false;
	for t in &toks {
		if t.kind == TokKind::Eof {
			break;
		}
		let text = token_text(t)?;
		if text.is_empty() {
			continue;
		}
		let first = text.as_bytes()[0];
		if let Some(p) = prev_end {
			if is_ident_char(p) && is_ident_char(first) {
				out.push(' ');
			} else if p == b'-' && first == b'-' {
				// `- -b` (binary minus + unary minus / negative number)
				// would glue into `--` and start a comment that eats the
				// rest of the line
				out.push(' ');
			} else if text == ".." && (p >= b'0' && p <= b'9') {
				// `1 .. 2` must stay apart: `1..2` lexes as the malformed
				// number `1.2` (Luau rejects it outright)
				out.push(' ');
			} else if prev_was_concat && (first == b'.' || (first >= b'0' && first <= b'9')) {
				// `..` glued to `.5` / `2` makes `...5` / `1.2` — malformed
				out.push(' ');
			}
		}
		out.push_str(&text);
		prev_end = text.as_bytes().last().copied();
		prev_was_concat = text == "..";
	}
	out.push('\n');
	Ok(out)
}

fn is_ident_char(b: u8) -> bool {
	(b >= b'a' && b <= b'z')
		|| (b >= b'A' && b <= b'Z')
		|| (b >= b'0' && b <= b'9')
		|| b == b'_'
}

fn token_text(t: &Token) -> Result<String, String> {
	match t.kind {
		TokKind::Name | TokKind::Punct | TokKind::Label => Ok(t.text.clone()),
		TokKind::Num => Ok(num_text(t.num, t.isfloat)),
		TokKind::Str => {
			// emit the literal EXACTLY as the printer wrote it (the lexer
			// captured the raw span) — re-encoding from decoded bytes could
			// re-introduce high-byte passthrough (CJK garbage) for
			// ciphertext chunks that happen to contain no control bytes
			Ok(t.text.clone())
		}
		TokKind::Interp => Err("minify: backtick interpolation reached output \
			(parser must desugar it before print)".into()),
		TokKind::Eof => Ok(String::new()),
	}
}

/// Must match the printer's numeric emission exactly (round-trip).
fn num_text(v: f64, isfloat: bool) -> String {
	if v.is_nan() {
		return "0.0/0.0".to_string();
	}
	if v.is_infinite() {
		return if v < 0.0 {
			"-math.huge".to_string()
		} else {
			"math.huge".to_string()
		};
	}
	if v.fract() == 0.0 && v.abs() < 1e15 && !isfloat {
		format!("{}", v as i64)
	} else {
		format!("{:?}", v)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	/// minify must not change the token sequence
	fn assert_same_tokens(src: &str, minified: &str) {
		let a = Lexer::new(src, false)
			.tokens()
			.unwrap()
			.iter()
			.map(token_text)
			.collect::<Result<Vec<_>, _>>()
			.unwrap();
		let b = Lexer::new(minified, false)
			.tokens()
			.unwrap()
			.iter()
			.map(token_text)
			.collect::<Result<Vec<_>, _>>()
			.unwrap();
		assert_eq!(a, b, "token sequence changed by minify");
	}

	#[test]
	fn basic_compaction() {
		let src = "local x = 1\nlocal y = x + 2\nif y > 2 then\n\tx = 0\nelse\n\tx = 1\nend\n";
		let m = minify(src, false).unwrap();
		assert!(m.trim_end().contains('\n') == false, "must be single line");
		assert_eq!(m.trim_end(), "local x=1 local y=x+2 if y>2 then x=0 else x=1 end");
		assert_same_tokens(src, &m);
	}

	#[test]
	fn merge_risky_boundaries_get_space() {
		let cases = [
			("local a and b", "local a and b"),
			("not not true", "not not true"),
			("else if", "else if"),
			("end do", "end do"),
			("return 1 end", "return 1 end"),
			("in i", "in i"),
			("then print", "then print"),
			("nil end", "nil end"),
		];
		for (src, want) in cases {
			let m = minify(src, false).unwrap();
			assert_eq!(m.trim_end(), want, "case: {src}");
			assert_same_tokens(src, &m);
		}
	}

	#[test]
	fn concat_number_boundaries_get_space() {
		let cases = [
			("1 .. 2", "1 .. 2"),
			("1 .. 2 .. 3", "1 .. 2 .. 3"),
			("x .. 2", "x.. 2"),
			("1 .. .5", "1 .. 0.5"), // .5 re-encodes as 0.5 (same value)
			("a .. b", "a..b"),
		];
		for (src, want) in cases {
			let m = minify(src, false).unwrap();
			assert_eq!(m.trim_end(), want, "case: {src}");
			assert_same_tokens(src, &m);
		}
	}

	#[test]
	fn double_minus_needs_space() {
		// `- - 5` and `a - -b`: gluing would start a `--` comment
		let cases = [
			("- - 5", "- -5"),
			("a - -b", "a- -b"),
			("-(-5)", "-(-5)"),
		];
		for (src, want) in cases {
			let m = minify(src, false).unwrap();
			assert_eq!(m.trim_end(), want, "case: {src}");
			assert_same_tokens(src, &m);
		}
	}

	#[test]
	fn gluable_boundaries_have_no_space() {
		let cases = [
			("f(x)end", "f(x)end"),
			("a..b", "a..b"),
			("a+b-c*d", "a+b-c*d"),
			("#t==0", "#t==0"),
			("v:m()", "v:m()"),
			("return...", "return..."),
		];
		for (src, want) in cases {
			let m = minify(src, false).unwrap();
			assert_eq!(m.trim_end(), want, "case: {src}");
			assert_same_tokens(src, &m);
		}
	}

	#[test]
	fn strings_roundtrip() {
		// decoded bytes: a, NUL, b, LF, ' ', '1' — re-encoded byte-exact
		// (NUL -> \0; LF not followed by a digit -> 2-digit \10; the
		// 3-digit zero-pad digit-merge guard is covered by printer tests)
		let src = "local s = \"a\\0b\\10 1\"\nprint(s)\n";
		let m = minify(src, false).unwrap();
		assert_same_tokens(src, &m);
		assert!(m.contains("\"a\\0b\\10 1\""), "got: {m}");
	}

	#[test]
	fn idempotent() {
		let src = std::fs::read_to_string(concat!(
			env!("CARGO_MANIFEST_DIR"), "/tests/cases/edge.lua"
		))
		.unwrap();
		// full pipeline input equivalent: minify twice via the lexer
		let m1 = minify(&src, false).unwrap();
		let m2 = minify(&m1, false).unwrap();
		assert_eq!(m1, m2, "minify is not idempotent");
	}

	#[test]
	fn numbers_roundtrip() {
		// forms the printer actually emits: integer / plain float / large
		// float (Rust {:?} renders 1.5e10 as 15000000000.0 — the printer
		// does too, so minify keeps it) / tiny float (scientific kept)
		let src = "local a = 42\nlocal b = 3.14\nlocal c = 1.5e10\nlocal d = 1e-7\nprint(a, b, c, d)\n";
		let m = minify(src, false).unwrap();
		assert_same_tokens(src, &m);
		assert_eq!(
			m.trim_end(),
			"local a=42 local b=3.14 local c=15000000000.0 local d=1e-7 print(a,b,c,d)"
		);
	}
}

