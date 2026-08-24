//! AST -> source printer.
//!
//! Precedence/paren rules mirror the Pratt parser exactly:
//! - Bin child of Bin: effective child prec = min(left,right prio);
//!   left side: parens if eff < parent.left;
//!   right side: parens if eff < parent.right, or (eff == parent.right and
//!   parent is left-associative).
//! - Unary child: right side of Bin -> never needs parens (parser accepts a
//!   unary operand at the right position); left side -> parens if 8 <
//!   parent.left; operand of Unary -> parens if eff < 8 (Un child: never).
//! - This reproduces: -2^2 = -(2^2); 2^-3; (a..b)..c; 10/(3/2); 2^3^2 ...

use crate::ast::*;
use crate::symtab::SymTable;

pub struct Printer<'a> {
	pub table: &'a SymTable,
	pub out: String,
	pub indent: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Ctx {
	/// no parent (statement level, table field, call arg, etc.)
	Top,
	/// operand of a binary op; `right` marks the right-hand side
	Bin(BinOp, bool),
	/// operand of a unary op
	Unary,
	/// base of a method call or function call — literals and non-atomic
	/// expressions need parens (`"x":m()`, `("a" .. "b"):len()`)
	Base,
}

impl<'a> Printer<'a> {
	pub fn new(table: &'a SymTable) -> Printer<'a> {
		Printer {
			table,
			out: String::new(),
			indent: 0,
		}
	}

	fn ind(&self) -> String {
		"    ".repeat(self.indent)
	}

	pub fn print_block(&mut self, b: &Block) {
		for (i, s) in b.stmts.iter().enumerate() {
			self.print_stmt(s);
			if i + 1 < b.stmts.len() {
				self.out.push('\n');
			}
		}
	}

	fn print_stmt(&mut self, s: &Stmt) {
		match s {
			Stmt::Local { names, syms, values } => {
				self.out.push_str(&self.ind());
				self.out.push_str("local ");
				for i in 0..names.len() {
					if i > 0 {
						self.out.push_str(", ");
					}
					self.out.push_str(self.table.name_of(syms[i]));
				}
				let any_val = values.iter().any(|v| v.is_some());
				if any_val {
					self.out.push_str(" = ");
					for (i, v) in values.iter().enumerate() {
						if i > 0 {
							self.out.push_str(", ");
						}
						match v {
							Some(e) => self.print_expr(e, Ctx::Top),
							None => self.out.push_str("nil"),
						}
					}
				}
			}
			Stmt::LocalFunc { name, sym, func } => {
				let _ = name;
				self.out.push_str(&self.ind());
				self.out.push_str("local function ");
				self.out.push_str(self.table.name_of(*sym));
				self.print_func_signature(func);
				self.indent += 1;
				self.out.push('\n');
				self.print_block(&func.body);
				self.indent -= 1;
				self.out.push('\n');
				self.out.push_str(&self.ind());
				self.out.push_str("end");
			}
			Stmt::FuncDecl { obj, name, ismethod, func } => {
				self.out.push_str(&self.ind());
				self.out.push_str("function ");
				if let Some(o) = obj {
					self.print_expr(o, Ctx::Base);
					self.out.push(if *ismethod { ':' } else { '.' });
				}
				self.out.push_str(name);
				self.print_func_signature(func);
				self.indent += 1;
				self.out.push('\n');
				self.print_block(&func.body);
				self.indent -= 1;
				self.out.push('\n');
				self.out.push_str(&self.ind());
				self.out.push_str("end");
			}
			Stmt::Assign { targets, values } => {
				self.out.push_str(&self.ind());
				for (i, t) in targets.iter().enumerate() {
					if i > 0 {
						self.out.push_str(", ");
					}
					self.print_expr(t, Ctx::Top);
				}
				self.out.push_str(" = ");
				for (i, v) in values.iter().enumerate() {
					if i > 0 {
						self.out.push_str(", ");
					}
					self.print_expr(v, Ctx::Top);
				}
			}
			Stmt::ExprStmt(e) => {
				self.out.push_str(&self.ind());
				self.print_expr(e, Ctx::Top);
			}
			Stmt::If { cond, thenb, elsifs, elseb } => {
				self.out.push_str(&self.ind());
				self.out.push_str("if ");
				self.print_expr(cond, Ctx::Top);
				self.out.push_str(" then");
				self.indent += 1;
				self.out.push('\n');
				self.print_block(thenb);
				for (c, b) in elsifs {
					self.indent -= 1;
					self.out.push('\n');
					self.out.push_str(&self.ind());
					self.out.push_str("elseif ");
					self.print_expr(c, Ctx::Top);
					self.out.push_str(" then");
					self.indent += 1;
					self.out.push('\n');
					self.print_block(b);
				}
				if let Some(b) = elseb {
					self.indent -= 1;
					self.out.push('\n');
					self.out.push_str(&self.ind());
					self.out.push_str("else");
					self.indent += 1;
					self.out.push('\n');
					self.print_block(b);
				}
				self.indent -= 1;
				self.out.push('\n');
				self.out.push_str(&self.ind());
				self.out.push_str("end");
			}
			Stmt::While { cond, body } => {
				self.out.push_str(&self.ind());
				self.out.push_str("while ");
				self.print_expr(cond, Ctx::Top);
				self.out.push_str(" do");
				self.indent += 1;
				self.out.push('\n');
				self.print_block(body);
				self.indent -= 1;
				self.out.push('\n');
				self.out.push_str(&self.ind());
				self.out.push_str("end");
			}
			Stmt::Repeat { body, cond } => {
				self.out.push_str(&self.ind());
				self.out.push_str("repeat");
				self.indent += 1;
				self.out.push('\n');
				self.print_block(body);
				self.indent -= 1;
				self.out.push('\n');
				self.out.push_str(&self.ind());
				self.out.push_str("until ");
				self.print_expr(cond, Ctx::Top);
			}
			Stmt::ForNum { var, var_sym, start, limit, step, body } => {
				self.out.push_str(&self.ind());
				self.out.push_str("for ");
				self.out.push_str(self.table.name_of(*var_sym));
				self.out.push_str(" = ");
				self.print_expr(start, Ctx::Top);
				self.out.push_str(", ");
				self.print_expr(limit, Ctx::Top);
				if let Some(st) = step {
					self.out.push_str(", ");
					self.print_expr(st, Ctx::Top);
				}
				self.out.push_str(" do");
				self.indent += 1;
				self.out.push('\n');
				self.print_block(body);
				self.indent -= 1;
				self.out.push('\n');
				self.out.push_str(&self.ind());
				self.out.push_str("end");
			}
			Stmt::ForGen { vars, syms, iters, body } => {
				self.out.push_str(&self.ind());
				self.out.push_str("for ");
				for (i, id) in syms.iter().enumerate() {
					if i > 0 {
						self.out.push_str(", ");
					}
					let _ = vars;
					self.out.push_str(self.table.name_of(*id));
				}
				self.out.push_str(" in ");
				for (i, it) in iters.iter().enumerate() {
					if i > 0 {
						self.out.push_str(", ");
					}
					self.print_expr(it, Ctx::Top);
				}
				self.out.push_str(" do");
				self.indent += 1;
				self.out.push('\n');
				self.print_block(body);
				self.indent -= 1;
				self.out.push('\n');
				self.out.push_str(&self.ind());
				self.out.push_str("end");
			}
			Stmt::Do(b) => {
				self.out.push_str(&self.ind());
				self.out.push_str("do");
				self.indent += 1;
				self.out.push('\n');
				self.print_block(b);
				self.indent -= 1;
				self.out.push('\n');
				self.out.push_str(&self.ind());
				self.out.push_str("end");
			}
			Stmt::Break => {
				self.out.push_str(&self.ind());
				self.out.push_str("break");
			}
			Stmt::Continue => {
				self.out.push_str(&self.ind());
				self.out.push_str("continue");
			}
			Stmt::Return(es) => {
				self.out.push_str(&self.ind());
				if es.is_empty() {
					self.out.push_str("return");
				} else {
					self.out.push_str("return ");
					for (i, e) in es.iter().enumerate() {
						if i > 0 {
							self.out.push_str(", ");
						}
						self.print_expr(e, Ctx::Top);
					}
				}
			}
		}
	}

	fn print_func_signature(&mut self, f: &FuncDef) {
		self.out.push('(');
		// for method declarations (`:name`), params[0] is the implicit `self`
		// provided by the `:` syntax — the body references it by the fixed
		// name `self` (keep_name), so skip it in the printed signature
		let start = if f.has_self { 1 } else { 0 };
		for i in start..f.params.len() {
			if i > start {
				self.out.push_str(", ");
			}
			let name = if i < f.param_syms.len() {
				self.table.name_of(f.param_syms[i])
			} else {
				f.params[i].as_str()
			};
			self.out.push_str(name);
		}
		if f.vararg {
			if !f.params.is_empty() {
				self.out.push_str(", ");
			}
			self.out.push_str("...");
		}
		self.out.push(')');
		self.out.push(' ');
	}

	fn print_expr(&mut self, e: &Expr, ctx: Ctx) {
		if needs_parens(e, ctx) {
			self.out.push('(');
			self.print_expr_inner(e, Ctx::Top);
			self.out.push(')');
			return;
		}
		self.print_expr_inner(e, ctx);
	}

	fn print_expr_inner(&mut self, e: &Expr, ctx: Ctx) {
		match e {
			Expr::Num { value, isfloat } => self.print_num(*value, *isfloat),
			Expr::Str { bytes, is_binary } => {
				self.out.push('"');
				self.out.push_str(&print_string_bytes(bytes, *is_binary));
				self.out.push('"');
			}
			Expr::Bool { value } => {
				self.out.push_str(if *value { "true" } else { "false" });
			}
			Expr::Nil => self.out.push_str("nil"),
			Expr::Vararg => self.out.push_str("..."),
			Expr::Ident { name, sym } => match sym {
				Some(id) => self.out.push_str(self.table.name_of(*id)),
				None => self.out.push_str(name),
			},
			Expr::Dot { obj, name } => {
				self.print_expr(obj, ctx);
				self.out.push('.');
				self.out.push_str(name);
			}
			Expr::Index { obj, idx } => {
				self.print_expr(obj, ctx);
				self.out.push('[');
				self.print_expr(idx, Ctx::Top);
				self.out.push(']');
			}
			Expr::Call { func, args } => {
				self.print_expr(func, Ctx::Base);
				self.print_args(args);
			}
			Expr::Method { obj, name, args } => {
				self.print_expr(obj, Ctx::Base);
				self.out.push(':');
				self.out.push_str(name);
				self.print_args(args);
			}
			Expr::Un { op, e } => {
				self.out.push_str(op.text());
				self.out.push(' ');
				self.print_expr(e, Ctx::Unary);
			}
			Expr::Bin { op, l, r } => {
			if *op == BinOp::Idiv {
				// // desugared to math.floor(l / r) — valid in both
				// dialects. Operands need Bin-context parens:
				// (a + b) // c must print math.floor((a + b) / c)
				self.out.push_str("math.floor(");
				self.print_expr(l, Ctx::Bin(BinOp::Div, false));
				self.out.push_str(" / ");
				self.print_expr(r, Ctx::Bin(BinOp::Div, true));
				self.out.push(')');
			} else {
					self.print_expr(l, Ctx::Bin(*op, false));
					self.out.push(' ');
					self.out.push_str(op.text());
					self.out.push(' ');
					self.print_expr(r, Ctx::Bin(*op, true));
				}
			}
			Expr::Table { fields } => {
				self.out.push('{');
				for (i, f) in fields.iter().enumerate() {
					if i > 0 {
						self.out.push_str(", ");
					}
					match f {
						TableField::Array(v) => self.print_expr(v, Ctx::Top),
						TableField::Key { key, value } => {
							if let Expr::Str { bytes, .. } = key {
								if is_printable_key(bytes) {
									self.out.push_str(&String::from_utf8_lossy(bytes));
									self.out.push_str(" = ");
									self.print_expr(value, Ctx::Top);
								} else {
									self.out.push('[');
									self.print_expr(key, Ctx::Top);
									self.out.push_str("] = ");
									self.print_expr(value, Ctx::Top);
								}
							} else {
								self.out.push('[');
								self.print_expr(key, Ctx::Top);
								self.out.push_str("] = ");
								self.print_expr(value, Ctx::Top);
							}
						}
					}
				}
				self.out.push('}');
			}
			Expr::Function { params, param_syms, vararg, body } => {
				self.out.push_str("function(");
				for (i, p) in params.iter().enumerate() {
					if i > 0 {
						self.out.push_str(", ");
					}
					let name = if i < param_syms.len() {
						self.table.name_of(param_syms[i])
					} else {
						p.as_str()
					};
					self.out.push_str(name);
				}
				if *vararg {
					if !params.is_empty() {
						self.out.push_str(", ");
					}
					self.out.push_str("...");
				}
				self.out.push_str(")");
				self.indent += 1;
				self.out.push('\n');
				self.print_block(body);
				self.indent -= 1;
				self.out.push('\n');
				self.out.push_str(&self.ind());
				self.out.push_str("end");
			}
		}
	}

	fn print_args(&mut self, args: &[Expr]) {
		match args {
			[] => {
				self.out.push_str("()");
			}
			[single] if matches!(single, Expr::Table { .. }) => {
				// f{...}  (parenless table form)
				self.print_expr(single, Ctx::Top);
			}
			_ => {
				self.out.push('(');
				for (i, a) in args.iter().enumerate() {
					if i > 0 {
						self.out.push_str(", ");
					}
					self.print_expr(a, Ctx::Top);
				}
				self.out.push(')');
			}
		}
	}

	fn print_num(&mut self, v: f64, isfloat: bool) {
		if v.is_nan() {
			self.out.push_str("0.0/0.0");
			return;
		}
		if v.is_infinite() {
			if v < 0.0 {
				self.out.push_str("-math.huge");
			} else {
				self.out.push_str("math.huge");
			}
			return;
		}
		let is_int = v.fract() == 0.0 && v.abs() < 1e15;
		if is_int && !isfloat {
			self.out.push_str(&format!("{}", v as i64));
		} else {
			// shortest round-trip; Rust's f64 Debug always keeps a '.' or 'e'
			self.out.push_str(&format!("{:?}", v));
		}
	}
}

fn is_printable_key(b: &[u8]) -> bool {
	!b.is_empty()
		&& std::str::from_utf8(b).is_ok()
		&& b.iter().all(|&c| {
			c == b'_'
				|| (c >= b'a' && c <= b'z')
				|| (c >= b'A' && c <= b'Z')
				|| (c >= b'1' && c <= b'9')
		})
		&& b[0] != b'0'
}

/// Byte-exact string literal content.
///
/// `is_binary` (ciphertext / key-stream / VM bytecode blobs): EVERY
/// non-printable-ASCII byte is \ddd-escaped — no UTF-8 passthrough, which
/// would otherwise emit random CJK garbage from arbitrary ciphertext bytes
/// (high bytes that happen to form valid UTF-8, e.g. E8 AF 9C = 嘱).
///
/// `!is_binary` (user strings): UTF-8-aware — valid printable UTF-8 passes
/// through (readable 你好 stays readable); everything else is escaped.
///
/// IMPORTANT: a decimal escape followed by a literal digit byte would merge
/// on re-lex (`\1` + `2` => `\12`). When the next byte is a digit we emit a
/// 3-digit zero-padded escape (`\001`) so the lexer stops at exactly 3 digits.
pub fn print_string_bytes(bytes: &[u8], is_binary: bool) -> String {
	let mut out = String::new();
	let mut i = 0usize;
	while i < bytes.len() {
		let b = bytes[i];
		let next_is_digit = bytes.get(i + 1).map_or(false, |&n| n >= b'0' && n <= b'9');
		if b < 0x80 {
			match b {
				b'"' => out.push_str("\\\""),
				b'\\' => out.push_str("\\\\"),
				c if (0x20..0x7f).contains(&c) => out.push(c as char),
				_ => {
					if next_is_digit {
						out.push_str(&format!("\\{:03}", b));
					} else {
						out.push_str(&format!("\\{}", b));
					}
				}
			}
			i += 1;
		} else if is_binary {
			// ciphertext: raw high bytes must never reach the literal
			if next_is_digit {
				out.push_str(&format!("\\{:03}", b));
			} else {
				out.push_str(&format!("\\{}", b));
			}
			i += 1;
		} else {
			// try to decode a UTF-8 sequence
			let len = match b {
				0xC0..=0xDF => 2,
				0xE0..=0xEF => 3,
				0xF0..=0xF7 => 4,
				_ => 1,
			};
			if i + len <= bytes.len() && std::str::from_utf8(&bytes[i..i + len]).is_ok() {
				let ch = std::str::from_utf8(&bytes[i..i + len]).unwrap().chars().next().unwrap();
				if ch.is_control() || ch == '"' || ch == '\\' {
					for k in i..i + len {
						out.push_str(&format!("\\{}", bytes[k]));
					}
				} else {
					out.push(ch);
				}
				i += len;
			} else {
				out.push_str(&format!("\\{}", b));
				i += 1;
			}
		}
	}
	out
}

/// Parenthesis decision. Returns true when `e` must be wrapped in parens
/// inside `ctx`.
fn needs_parens(e: &Expr, ctx: Ctx) -> bool {
	// base of a call: anything that is not an atomic prefixexp needs parens
	if let Ctx::Base = ctx {
		match e {
			Expr::Ident { .. }
			| Expr::Dot { .. }
			| Expr::Index { .. }
			| Expr::Call { .. }
			| Expr::Method { .. }
			| Expr::Table { .. } => return false,
			_ => return true,
		}
	}
	match (e, ctx) {
		(Expr::Bin { op: cop, .. }, Ctx::Bin(parent, right)) => {
			let (lp, rp) = cop.prio();
			let eff = lp.min(rp);
			let (plp, prp) = parent.prio();
			let right_assoc = prp < plp;
			if right {
				eff < prp || (eff == prp && !right_assoc)
			} else {
				eff < plp
			}
		}
		(Expr::Bin { op: cop, .. }, Ctx::Unary) => {
			let (lp, rp) = cop.prio();
			lp.min(rp) < 8
		}
		(Expr::Un { .. }, Ctx::Bin(_, right)) => {
			// right side: parser always accepts a unary operand here
			// (2^-3, a and not b); left side: treat as prec 8
			if right {
				false
			} else {
				let (plp, _) = match ctx {
					Ctx::Bin(op, _) => op.prio(),
					_ => (0, 0),
				};
				8 < plp
			}
		}
		(Expr::Un { .. }, Ctx::Unary) => false,
		_ => false,
	}
}

/// Public entry: print a whole chunk.
pub fn print_chunk(table: &SymTable, block: &Block) -> String {
	let mut p = Printer::new(table);
	p.print_block(block);
	p.out.push('\n');
	p.out
}
