//! Byte-oriented lexer for Lua 5.1 and Luau source.
//!
//! Handles: numbers (dec/hex, isfloat flag), short strings with all escapes
//! (\n \t \r \a \b \f \v \\ \" \' \ddd \xhh, line continuation), long strings
//! ([[ ]], [= [= ]= ]]), comments (short + long), punctuation, keywords,
//! labels (rejected later by the parser), Luau-only: `//`, compound
//! assignments (`+=` ...), backtick string interpolation `` `a {e} b` ``.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokKind {
	Name,
	Num,
	Str,
	Punct,
	Label,
	Interp,
	Eof,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterpPart {
	Text,
	Expr,
}

#[derive(Debug, Clone)]
pub struct Token {
	/// Name / Punct / Label text (keywords are plain names; the parser does
	/// keyword dispatch contextually, like the reference implementations).
	pub text: String,
	pub num: f64,
	pub isfloat: bool,
	/// Str: decoded bytes.
	/// Interp: text parts encoded as [4-byte LE length][bytes]...
	pub bytes: Vec<u8>,
	/// Interp: part kinds in order.
	pub parts: Vec<InterpPart>,
	/// Interp: raw source of each Expr part, in order.
	pub interp_srcs: Vec<String>,
	pub line: usize,
	pub kind: TokKind,
}

#[derive(Debug)]
pub struct LexError {
	pub line: usize,
	pub msg: String,
}

impl fmt::Display for LexError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "lex error line {}: {}", self.line, self.msg)
	}
}

impl std::error::Error for LexError {}

pub struct Lexer {
	src: Vec<u8>,
	pos: usize,
	line: usize,
	luau: bool,
}

const KEYWORDS: &[&str] = &[
	"and", "break", "do", "else", "elseif", "end", "false", "for", "function", "if", "in",
	"local", "nil", "not", "or", "repeat", "return", "then", "true", "until", "while",
];

fn is_digit(b: u8) -> bool {
	b >= b'0' && b <= b'9'
}
fn is_hex(b: u8) -> bool {
	is_digit(b) || (b >= b'a' && b <= b'f') || (b >= b'A' && b <= b'F')
}
fn is_alpha(b: u8) -> bool {
	(b >= b'a' && b <= b'z') || (b >= b'A' && b <= b'Z') || b == b'_'
}
fn is_alnum(b: u8) -> bool {
	is_alpha(b) || is_digit(b)
}

impl Lexer {
	pub fn new(src: &str, luau: bool) -> Lexer {
		Lexer {
			src: src.as_bytes().to_vec(),
			pos: 0,
			line: 1,
			luau,
		}
	}

	fn err(&self, msg: &str) -> LexError {
		LexError {
			line: self.line,
			msg: msg.to_string(),
		}
	}

	fn peek(&self, off: usize) -> u8 {
		*self.src.get(self.pos + off).unwrap_or(&0)
	}

	fn peek_at(&self, abs: usize) -> u8 {
		*self.src.get(abs).unwrap_or(&0)
	}

	fn advance(&mut self, n: usize) {
		for i in 0..n {
			if self.peek_at(self.pos + i) == b'\n' {
				self.line += 1;
			}
		}
		self.pos += n;
	}

	fn count_newlines(&self, from: usize, to: usize) -> usize {
		self.src[from..to.min(self.src.len())].iter().filter(|&&b| b == b'\n').count()
	}

	fn skip_ws_and_comments(&mut self) -> Result<(), LexError> {
		loop {
			let c = self.peek(0);
			match c {
				b' ' | b'\t' | b'\r' | b'\n' => self.advance(1),
				b'-' if self.peek(1) == b'-' => {
					let mut p = self.pos + 2;
					let mut lvl = 0usize;
					while self.peek_at(p) == b'=' {
						lvl += 1;
						p += 1;
					}
					if self.peek_at(p) == b'[' {
						let close = format!("]{}]", "=".repeat(lvl));
						let cb = close.as_bytes();
						match self.src.windows(cb.len()).skip(p + 1).position(|w| w == cb) {
							Some(rel) => {
								let q = p + 1 + rel;
								self.line += self.count_newlines(self.pos, q + cb.len());
								self.pos = q + cb.len();
							}
							None => return Err(self.err("unterminated comment")),
						}
					} else {
						match self.src[self.pos..].iter().position(|&b| b == b'\n') {
							Some(off) => self.pos = self.pos + off,
							None => self.pos = self.src.len(),
						}
					}
				}
				_ => break,
			}
		}
		Ok(())
	}

	fn lex_number(&mut self) -> Result<Token, LexError> {
		let start = self.pos;
		let c0 = self.peek(0);
		let c1 = self.peek(1);
		let mut isfloat = false;
		let end;
		if c0 == b'0' && (c1 == b'x' || c1 == b'X') {
			let mut p = self.pos + 2;
			while is_hex(self.peek_at(p)) {
				p += 1;
			}
			if p == self.pos + 2 {
				return Err(self.err("malformed hex number"));
			}
			end = p;
		} else {
			let mut p = self.pos;
			let mut seen_dot = false;
			while is_digit(self.peek_at(p)) {
				p += 1;
			}
			if self.peek_at(p) == b'.' {
				seen_dot = true;
				p += 1;
				while is_digit(self.peek_at(p)) {
					p += 1;
				}
			}
			if self.peek_at(p) == b'e' || self.peek_at(p) == b'E' {
				let mut q = p + 1;
				if self.peek_at(q) == b'+' || self.peek_at(q) == b'-' {
					q += 1;
				}
				let mut exp_digits = false;
				while is_digit(self.peek_at(q)) {
					exp_digits = true;
					q += 1;
				}
				if !exp_digits {
					return Err(self.err("malformed number exponent"));
				}
				isfloat = true;
				p = q;
			}
			if p == self.pos {
				return Err(self.err("malformed number"));
			}
			isfloat = isfloat || seen_dot;
			end = p;
		}
		let raw = self.src[start..end].to_vec();
		self.advance(end - self.pos);
		let text = std::str::from_utf8(&raw).map_err(|_| self.err("bad number"))?;
		let num: f64 = if text.starts_with("0x") || text.starts_with("0X") {
			let v = u64::from_str_radix(&text[2..], 16).map_err(|_| self.err("bad hex number"))?;
			v as f64
		} else {
			// normalize Lua forms Rust doesn't accept: "5." -> "5.0", ".5" -> "0.5"
			let mut norm = String::new();
			if text.starts_with('.') {
				norm.push('0');
			}
			norm.push_str(text);
			if norm.ends_with('.') {
				norm.push('0');
			}
			norm.parse().map_err(|_| self.err("bad number"))?
		};
		Ok(self.mktok(TokKind::Num, text.to_string(), num, isfloat))
	}

	/// Escape decoding shared by short strings and backtick text sections.
	/// `pos` is at the backslash. Returns true when the caller must NOT do its
	/// default advance(2).
	///
	/// Dialect difference (verified against real 5.1.5 and Luau 0.735):
	/// - Lua 5.1: `\x` is NOT a hex escape (it yields literal 'x'); any
	///   unknown escape `\c` yields the literal character c.
	/// - Luau: `\xhh` is a hex escape (1-2 digits); unknown escapes error.
	fn decode_escape(&mut self, out: &mut Vec<u8>) -> Result<bool, LexError> {
		let e = self.peek(1);
		match e {
			b'\n' => {
				self.advance(1); // the backslash
				self.skip_line_continuation()?;
				return Ok(true);
			}
			b'n' => out.push(b'\n'),
			b't' => out.push(b'\t'),
			b'r' => out.push(b'\r'),
			b'a' => out.push(7),
			b'b' => out.push(8),
			b'f' => out.push(12),
			b'v' => out.push(11),
			b'\\' => out.push(b'\\'),
			b'`' => out.push(b'`'),
			b'{' => out.push(b'{'),
			b'}' => out.push(b'}'),
			b'"' => out.push(b'"'),
			b'\'' => out.push(b'\''),
			d if is_digit(d) => {
				let mut dstr = String::new();
				let mut q = self.pos + 1;
				for _ in 0..3 {
					if is_digit(self.peek_at(q)) {
						dstr.push(self.peek_at(q) as char);
						q += 1;
					} else {
						break;
					}
				}
				let v: u32 = dstr.parse().map_err(|_| self.err("bad escape"))?;
				if v > 255 {
					return Err(self.err("escape value > 255"));
				}
				out.push(v as u8);
				self.advance(1 + dstr.len());
				return Ok(true);
			}
			b'x' if self.luau => {
				// Luau only: \xhh hex escape — exactly two hex digits
				let h = self.peek_at(self.pos + 2);
				let h2 = self.peek_at(self.pos + 3);
				if !is_hex(h) || !is_hex(h2) {
					return Err(self.err("bad \\x escape (needs two hex digits)"));
				}
				let hexs = String::from_utf8_lossy(&self.src[self.pos + 2..self.pos + 4]);
				let v = u8::from_str_radix(&hexs, 16).map_err(|_| self.err("bad \\x escape"))?;
				out.push(v);
				self.advance(4);
				return Ok(true);
			}
			_ => {
				if self.luau {
					return Err(self.err("invalid escape sequence"));
				}
				// Lua 5.1: unknown escape (including \x) -> literal character
				out.push(e);
				return Ok(false);
			}
		}
		Ok(false)
	}

	fn skip_line_continuation(&mut self) -> Result<(), LexError> {
		// pos is right after the backslash
		loop {
			let c = self.peek(0);
			if c == b'\n' {
				self.advance(1);
				break;
			} else if c == b' ' || c == b'\t' || c == b'\r' {
				self.advance(1);
			} else if c == b'-' && self.peek(1) == b'-' {
				match self.src[self.pos..].iter().position(|&b| b == b'\n') {
					Some(off) => self.pos = self.pos + off,
					None => {
						self.pos = self.src.len();
						break;
					}
				}
			} else {
				break;
			}
		}
		Ok(())
	}

	fn lex_short_string(&mut self, quote: u8) -> Result<Token, LexError> {
		let line = self.line;
		self.advance(1); // opening quote
		let mut out: Vec<u8> = Vec::new();
		loop {
			let c = self.peek(0);
			if c == 0 {
				return Err(self.err("unterminated string"));
			}
			if c == quote {
				self.advance(1);
				break;
			}
			if c == b'\n' {
				return Err(self.err("unterminated string (newline)"));
			}
			if c == b'\\' {
				let skip = self.decode_escape(&mut out)?;
				if !skip {
					self.advance(2);
				}
			} else {
				out.push(c);
				self.advance(1);
			}
		}
		let mut t = self.mktok(TokKind::Str, String::new(), 0.0, false);
		t.bytes = out;
		t.line = line;
		Ok(t)
	}

	fn try_long_string_open(&self) -> Option<usize> {
		if self.peek(0) != b'[' {
			return None;
		}
		let mut lvl = 0usize;
		let mut p = self.pos + 1;
		while self.peek_at(p) == b'=' {
			lvl += 1;
			p += 1;
		}
		if self.peek_at(p) == b'[' {
			Some(lvl)
		} else {
			None
		}
	}

	fn lex_long_string(&mut self, lvl: usize) -> Result<Token, LexError> {
		let line = self.line;
		let open = format!("[{}[", "=".repeat(lvl));
		let close = format!("]{}]", "=".repeat(lvl));
		let cb = close.as_bytes();
		let start = self.pos + open.len();
		match self.src.windows(cb.len()).skip(start).position(|w| w == cb) {
			Some(rel) => {
				let q = start + rel;
				let mut body = self.src[start..q].to_vec();
				if body.first() == Some(&b'\n') {
					body.remove(0);
				}
				self.line += self.count_newlines(self.pos, q + cb.len());
				self.pos = q + cb.len();
				let mut t = self.mktok(TokKind::Str, String::new(), 0.0, false);
				t.bytes = body;
				t.line = line;
				Ok(t)
			}
			None => Err(self.err("unterminated long string")),
		}
	}

	/// Luau backtick string: `text {expr} text`. `{` starts a placeholder
	/// (balanced to the matching `}`, strings/comments inside skipped);
	/// `\{` is a literal brace. No placeholders => plain Str token.
	fn lex_backtick_string(&mut self) -> Result<Token, LexError> {
		let line = self.line;
		self.advance(1); // opening backtick
		let mut parts: Vec<InterpPart> = Vec::new();
		let mut interp_srcs: Vec<String> = Vec::new();
		let mut texts: Vec<Vec<u8>> = Vec::new();
		let mut cur: Vec<u8> = Vec::new();
		let mut has_expr = false;

		macro_rules! flush_text {
			() => {
				if !cur.is_empty() || parts.is_empty() {
					parts.push(InterpPart::Text);
					texts.push(std::mem::take(&mut cur));
				}
			};
		}

		loop {
			let c = self.peek(0);
			if c == 0 {
				return Err(self.err("unterminated interpolated string"));
			}
			if c == b'\n' || c == b'\r' {
				return Err(self.err("unterminated interpolated string (newline)"));
			}
			if c == b'`' {
				self.advance(1);
				break;
			}
			if c == b'\\' {
				let skip = self.decode_escape(&mut cur)?;
				if !skip {
					self.advance(2);
				}
			} else if c == b'{' {
				flush_text!();
				has_expr = true;
				self.advance(1); // consume '{'
				let expr_start = self.pos;
				let mut depth: i32 = 1;
				let mut q = self.pos;
				loop {
					if q >= self.src.len() {
						return Err(self.err("unterminated interpolated string"));
					}
					let ch = self.src[q];
					if ch == b'\n' {
						return Err(self.err("unterminated interpolated string (newline)"));
					}
					if ch == b'"' || ch == b'\'' {
						let quote = ch;
						q += 1;
						while q < self.src.len() {
							if self.src[q] == b'\\' {
								q += 2;
								continue;
							}
							if self.src[q] == quote {
								q += 1;
								break;
							}
							q += 1;
						}
						continue;
					} else if ch == b'[' && matches!(self.src.get(q + 1), Some(b'[') | Some(b'='))
					{
						let mut lvl = 0usize;
						let mut p2 = q + 1;
						while self.peek_at(p2) == b'=' {
							lvl += 1;
							p2 += 1;
						}
						if self.peek_at(p2) == b'[' {
							let close = format!("]{}]", "=".repeat(lvl));
							let cbb = close.as_bytes();
							match self.src.windows(cbb.len()).skip(p2 + 1).position(|w| w == cbb) {
								Some(rel) => {
									q = p2 + 1 + rel + cbb.len();
									continue;
								}
								None => return Err(self.err("unterminated long string")),
							}
						}
						q += 1;
					} else if ch == b'-' && self.src.get(q + 1) == Some(&b'-') {
						let mut p2 = q + 2;
						let mut lvl = 0usize;
						while self.peek_at(p2) == b'=' {
							lvl += 1;
							p2 += 1;
						}
						if self.peek_at(p2) == b'[' {
							let close = format!("]{}]", "=".repeat(lvl));
							let cbb = close.as_bytes();
							match self.src.windows(cbb.len()).skip(p2 + 1).position(|w| w == cbb) {
								Some(rel) => {
									q = p2 + 1 + rel + cbb.len();
									continue;
								}
								None => return Err(self.err("unterminated comment")),
							}
						} else {
							match self.src[q..].iter().position(|&b| b == b'\n') {
								Some(off) => q += off,
								None => {
									q = self.src.len();
									break;
								}
							}
						}
					} else if ch == b'{' {
						depth += 1;
						q += 1;
					} else if ch == b'}' {
						depth -= 1;
						if depth == 0 {
							break;
						}
						q += 1;
					} else {
						q += 1;
					}
				}
				let expr_bytes = &self.src[expr_start..q];
				if expr_bytes.first() == Some(&b'{') {
					return Err(self.err(
						"double braces are not permitted within interpolated strings",
					));
				}
				interp_srcs.push(String::from_utf8_lossy(expr_bytes).into_owned());
				parts.push(InterpPart::Expr);
				self.pos = q + 1; // consume '}'
			} else {
				cur.push(c);
				self.advance(1);
			}
		}

		let mut t = self.mktok(TokKind::Str, String::new(), 0.0, false);
		t.line = line;
		if !has_expr {
			t.bytes = cur;
			return Ok(t);
		}
		flush_text!();
		let mut enc = Vec::new();
		for tx in &texts {
			enc.extend_from_slice(&(tx.len() as u32).to_le_bytes());
			enc.extend_from_slice(tx);
		}
		t.kind = TokKind::Interp;
		t.bytes = enc;
		t.parts = parts;
		t.interp_srcs = interp_srcs;
		Ok(t)
	}

	fn lex_name(&mut self) -> Result<Token, LexError> {
		let start = self.pos;
		let mut p = self.pos + 1;
		while is_alnum(self.peek_at(p)) {
			p += 1;
		}
		let word = String::from_utf8_lossy(&self.src[start..p]).into_owned();
		self.advance(p - self.pos);
		Ok(self.mktok(TokKind::Name, word, 0.0, false))
	}

	fn punct_tok(&self, s: &str) -> Token {
		self.mktok(TokKind::Punct, s.to_string(), 0.0, false)
	}

	fn mktok(&self, kind: TokKind, text: String, num: f64, isfloat: bool) -> Token {
		Token {
			text,
			num,
			isfloat,
			bytes: Vec::new(),
			parts: Vec::new(),
			interp_srcs: Vec::new(),
			line: self.line,
			kind,
		}
	}

	pub fn is_keyword(name: &str) -> bool {
		KEYWORDS.contains(&name)
	}

	pub fn tokens(&mut self) -> Result<Vec<Token>, LexError> {
		let mut toks: Vec<Token> = Vec::new();
		loop {
			self.skip_ws_and_comments()?;
			if self.pos >= self.src.len() {
				let mut t = self.mktok(TokKind::Eof, String::new(), 0.0, false);
				t.kind = TokKind::Eof;
				toks.push(t);
				break;
			}
			let c = self.peek(0);
			let c2 = self.peek(1);
			if is_alpha(c) {
				toks.push(self.lex_name()?);
			} else if is_digit(c) || (c == b'.' && is_digit(c2)) {
				toks.push(self.lex_number()?);
			} else if c == b'"' || c == b'\'' {
				toks.push(self.lex_short_string(c)?);
			} else if let Some(lvl) = self.try_long_string_open() {
				toks.push(self.lex_long_string(lvl)?);
			} else if c == b'`' {
				if !self.luau {
					return Err(self.err("backtick strings require the Luau dialect"));
				}
				toks.push(self.lex_backtick_string()?);
			} else if c == b':' && c2 == b':' {
				// try label ::name::
				let mut ok = false;
				let mut p = self.pos + 2;
				if p < self.src.len() && is_alpha(self.src[p]) {
					let start = p;
					while is_alnum(self.peek_at(p)) {
						p += 1;
					}
					if p + 1 < self.src.len() && self.src[p] == b':' && self.src[p + 1] == b':' {
						let name = String::from_utf8_lossy(&self.src[start..p]).into_owned();
						self.advance(p + 2 - self.pos);
						toks.push(self.mktok(TokKind::Label, name, 0.0, false));
						ok = true;
					}
				}
				if !ok {
					toks.push(self.punct_tok(":"));
					self.advance(1);
				}
			} else {
				let mut matched = false;
				for t in ["...", "..", "==", "~=", "<=", ">="] {
					if self.src.len() >= self.pos + t.len()
						&& &self.src[self.pos..self.pos + t.len()] == t.as_bytes()
					{
						toks.push(self.punct_tok(t));
						self.advance(t.len());
						matched = true;
						break;
					}
				}
				if matched {
					continue;
				}
				// compound operators first (so `//=` wins over `//`)
				let mut compound_done = false;
				if self.luau {
					for t in ["+=", "-=", "*=", "/=", "//=", "%=", "^="] {
						if self.src.len() >= self.pos + t.len()
							&& &self.src[self.pos..self.pos + t.len()] == t.as_bytes()
						{
							toks.push(self.punct_tok(t));
							self.advance(t.len());
							compound_done = true;
							break;
						}
					}
				} else if matches!(c, b'+' | b'-' | b'*' | b'/' | b'%' | b'^') && c2 == b'=' {
					return Err(self.err("compound assignment requires --dialect luau"));
				}
				if compound_done {
					continue;
				}
				if c == b'/' && c2 == b'/' {
					if !self.luau {
						return Err(self.err("'//' requires --dialect luau"));
					}
					toks.push(self.punct_tok("//"));
					self.advance(2);
					continue;
				}
				if matches!(
					c,
					b'=' | b'{' | b'}' | b'[' | b']' | b'(' | b')' | b'<' | b'>' | b'+'
						| b'-' | b'*' | b'/' | b'%' | b'^' | b'#' | b':' | b',' | b'.' | b';'
						| b'&' | b'|' | b'?'
				) {
					toks.push(self.punct_tok(&(c as char).to_string()));
					self.advance(1);
				} else {
					return Err(self.err(&format!("unexpected character {:?}", c as char)));
				}
			}
		}
		Ok(toks)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn toks(src: &str, luau: bool) -> Vec<Token> {
		let mut l = Lexer::new(src, luau);
		l.tokens().unwrap()
	}

	#[test]
	fn numbers() {
		let t = toks("0 123 1.5 .5 5. 1e10 0x1F 1.5e-3", false);
		assert_eq!(t[0].num, 0.0);
		assert!(!t[0].isfloat);
		assert_eq!(t[2].num, 1.5);
		assert!(t[2].isfloat);
		assert!(t[3].isfloat);
		assert!(t[4].isfloat);
		assert!(t[5].isfloat);
		assert_eq!(t[6].num, 31.0);
		assert!(!t[6].isfloat);
	}

	#[test]
	fn strings_escapes() {
		// common escapes (both dialects)
		let t = toks(r#""a\n\t\65\66\0""#, false);
		assert_eq!(t[0].bytes, b"a\n\tAB\0");
		// \x is a hex escape in Luau only
		let tl = toks(r#""\x41\x45""#, true);
		assert_eq!(tl[0].bytes, b"AE");
		// ...and literal 'x..' in Lua 5.1
		let t51 = toks(r#""\x41""#, false);
		assert_eq!(t51[0].bytes, b"x41");
	}

	#[test]
	fn long_strings() {
		let t = toks("local x = [[ab\ncd]] local y = [==[ef]==]", false);
		assert_eq!(t[3].bytes, b"ab\ncd");
		assert_eq!(t[7].bytes, b"ef");
	}

	#[test]
	fn comments() {
		let t = toks("-- hi\nx --[[ block\n ]] y", false);
		assert_eq!(t[0].text, "x");
	}

	#[test]
	fn backtick_interp() {
		let t = toks(r"`a {x + 1} b {} c`", true);
		assert_eq!(t[0].kind, TokKind::Interp);
		assert_eq!(
			t[0].parts,
			vec![
				InterpPart::Text,
				InterpPart::Expr,
				InterpPart::Text,
				InterpPart::Expr,
				InterpPart::Text
			]
		);
		assert_eq!(t[0].interp_srcs, vec!["x + 1", ""]);
	}

	#[test]
	fn backtick_literal_brace() {
		let t = toks(r"`a \{ b`", true);
		assert_eq!(t[0].kind, TokKind::Str);
		assert_eq!(t[0].bytes, b"a { b");
	}

	#[test]
	fn luau_ops() {
		let t = toks("a += b // c continue", true);
		assert_eq!(t[1].text, "+=");
		assert_eq!(t[3].text, "//");
	}

	#[test]
	fn line_continuation() {
		// backslash immediately followed by newline
		let t = toks("\"abc\\\n def\"", false);
		assert_eq!(t[0].bytes, b"abc def");
	}

	#[test]
	fn interp_empty_placeholder_text() {
		let t = toks(r"`{x}`", true);
		assert_eq!(t[0].kind, TokKind::Interp);
		// empty leading text part is recorded explicitly
		assert_eq!(t[0].parts, vec![InterpPart::Text, InterpPart::Expr]);
	}
}


