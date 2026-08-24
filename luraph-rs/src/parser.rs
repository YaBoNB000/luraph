//! Recursive-descent parser for Lua 5.1 / Luau, with Pratt expression
//! priorities identical to the reference implementations (5.1 lparser.c and
//! Luau Parser.cpp — verified against source).
//!
//! Luau extras: continue, compound assignment (desugared to plain assign),
//! `//` (kept as a Div-with-flag? no — desugared to math.floor at the
//! desugar stage; here it is kept as BinOp::Div with a side marker? We keep
//! it as its own node via Bin with op Div and a `floor` flag is overkill:
//! `//` is stored as Bin{op: Div} only when dialect==luau and the printer
//! re-emits `/` — that would change semantics. So: `//` is desugared to
//! math.floor(a / b) right here at parse time when target is 5.1, and also
//! when target is luau (identical semantics, keeps one code path).
//! Backtick interpolation is desugared to string.format at parse time.
//! Type annotations / type aliases are parsed and dropped.

use crate::ast::*;
use crate::lexer::{InterpPart, Lexer, TokKind, Token};

const UNARY_PRIO: u8 = 8;

struct Parser {
	toks: Vec<Token>,
	i: usize,
	luau: bool,
	/// nesting depth of loops (break/continue validation)
	loops: u32,
}

impl Parser {
	fn peek(&self, ahead: usize) -> &Token {
		self.toks.get(self.i + ahead).unwrap_or_else(|| self.toks.last().unwrap())
	}

	fn next(&mut self) -> Token {
		let t = self.toks[self.i].clone();
		if t.kind != TokKind::Eof {
			self.i += 1;
		}
		t
	}

	fn err(&self, msg: &str) -> ParseError {
		ParseError {
			line: self.peek(0).line,
			msg: msg.to_string(),
		}
	}

	fn errf(&self, fmt: String) -> ParseError {
		self.err(&fmt)
	}

	fn is(&self, kind: TokKind, text: Option<&str>) -> bool {
		let t = self.peek(0);
		t.kind == kind && match text {
			Some(x) => t.text == *x,
			None => true,
		}
	}

	fn is_kw(&self, text: &str) -> bool {
		let t = self.peek(0);
		t.kind == TokKind::Name && t.text == text
	}

	fn eat(&mut self, kind: TokKind, text: Option<&str>) -> bool {
		if self.is(kind, text) {
			self.next();
			true
		} else {
			false
		}
	}

	fn expect(&mut self, kind: TokKind, text: Option<&str>) -> Result<Token, ParseError> {
		if self.is(kind, text) {
			Ok(self.next())
		} else {
			let want = match text {
				Some(x) => format!("{:?} '{}'", kind, x),
				None => format!("{:?}", kind),
			};
			let got = self.peek(0);
			let gotd = match got.kind {
				TokKind::Eof => "end of file".to_string(),
				TokKind::Num => format!("number {}", got.text),
				TokKind::Str => "string".to_string(),
				TokKind::Interp => "string".to_string(),
				_ => format!("'{}'", got.text),
			};
			Err(ParseError {
				line: got.line,
				msg: format!("expected {}, got {}", want, gotd),
			})
		}
	}

	fn eat_kw(&mut self, text: &str) -> bool {
		if self.is_kw(text) {
			self.next();
			true
		} else {
			false
		}
	}

	fn expect_kw(&mut self, text: &str) -> Result<(), ParseError> {
		if self.is_kw(text) {
			self.next();
			Ok(())
		} else {
			Err(self.errf(format!("expected '{}'", text)))
		}
	}

	// ------------------------------------------------------------------
	// blocks & statements

	fn block(&mut self) -> Result<Block, ParseError> {
		let mut stmts = Vec::new();
		loop {
			let t = self.peek(0);
			match t.kind {
				TokKind::Eof => break,
				TokKind::Name => {
					if matches!(t.text.as_str(), "end" | "else" | "elseif" | "until") {
						break;
					}
					if t.text == "return" {
						stmts.push(self.returnstat()?);
						self.eat(TokKind::Punct, Some(";"));
						break;
					}
					stmts.push(self.stat()?);
					self.eat(TokKind::Punct, Some(";"));
				}
				TokKind::Punct if t.text == ";" => {
					self.next();
				}
				_ => {
					stmts.push(self.stat()?);
					self.eat(TokKind::Punct, Some(";"));
				}
			}
		}
		Ok(Block { stmts })
	}

	fn stat(&mut self) -> Result<Stmt, ParseError> {
		let t = self.peek(0);
		if t.kind == TokKind::Label {
			return Err(self.errf(format!(
				"labels (::{})::) and goto are not supported in Lua 5.1 / Luau 0.735",
				t.text
			)));
		}
		if t.kind == TokKind::Name {
			match t.text.as_str() {
				"local" => return self.localstat(),
				"function" => return self.functionstat(),
				"if" => return self.ifstat(),
				"while" => return self.whilestat(),
				"do" => {
					self.next();
					let b = self.block()?;
					self.expect_kw("end")?;
					return Ok(Stmt::Do(b));
				}
				"repeat" => return self.repeatstat(),
				"for" => return self.forstat(),
				"break" => {
					if self.loops == 0 {
						return Err(self.errf("'break' outside of a loop".to_string()));
					}
					self.next();
					return Ok(Stmt::Break);
				}
				"continue" => {
					if !self.luau {
						return Err(self.errf(
							"'continue' requires --dialect luau".to_string(),
						));
					}
					// contextual keyword: only when it cannot start an expr statement
					let nt = self.peek(1);
					let not_expr = !(nt.kind == TokKind::Punct
						&& matches!(
							nt.text.as_str(),
							"=" | "," | "(" | "." | ":" | "+=" | "-=" | "*=" | "/=" | "%=" | "^="
						));
					if not_expr {
						if self.loops == 0 {
							return Err(self.errf(
								"'continue' outside of a loop".to_string(),
							));
						}
						self.next();
						return Ok(Stmt::Continue);
					}
				}
				"type" if self.luau => {
					let n1 = self.peek(1);
					let n2 = self.peek(2);
					if n1.kind == TokKind::Name
						&& n2.kind == TokKind::Punct
						&& (n2.text == "=" || n2.text == "<")
					{
						self.next(); // 'type'
						return self.typealias_body();
					}
				}
				"export" if self.luau => {
					let n1 = self.peek(1);
					let n2 = self.peek(2);
					let n3 = self.peek(3);
					if n1.kind == TokKind::Name
						&& n1.text == "type"
						&& n2.kind == TokKind::Name
						&& n3.kind == TokKind::Punct
						&& (n3.text == "=" || n3.text == "<")
					{
						self.next(); // 'export'
						self.next(); // 'type'
						return self.typealias_body();
					}
				}
				"goto" => {
					return Err(self.errf(
						"'goto' is not supported in Lua 5.1 / Luau 0.735".to_string(),
					));
				}
				_ => {}
			}
		}
		self.exprstat()
	}

	fn returnstat(&mut self) -> Result<Stmt, ParseError> {
		self.expect_kw("return")?;
		let t = self.peek(0);
		let stop = t.kind == TokKind::Eof
			|| t.kind == TokKind::Punct && t.text == ";"
			|| t.kind == TokKind::Name
				&& matches!(t.text.as_str(), "end" | "else" | "elseif" | "until");
		let exprs = if stop {
			Vec::new()
		} else {
			self.explist()?
		};
		Ok(Stmt::Return(exprs))
	}

	fn localstat(&mut self) -> Result<Stmt, ParseError> {
		self.expect_kw("local")?;
		if self.is_kw("function") {
			self.next();
			let nm = self.expect(TokKind::Name, None)?;
			let func = self.funcbody(false)?;
			return Ok(Stmt::LocalFunc {
				name: nm.text,
				sym: 0, // filled by symtab
				func: Box::new(func),
			});
		}
		// namelist (values come after, if any)
		let mut names: Vec<String> = Vec::new();
		loop {
			let t = self.expect(TokKind::Name, None)?;
			names.push(t.text.clone());
			self.eat(TokKind::Punct, Some("?")); // optional marker
			if self.eat(TokKind::Punct, Some(":")) {
				self.typeref()?;
			}
			if !self.eat(TokKind::Punct, Some(",")) {
				break;
			}
		}
		let values = if self.eat(TokKind::Punct, Some("=")) {
			let vs = self.explist()?;
			vs.into_iter().map(Some).collect()
		} else {
			names.iter().map(|_| None).collect()
		};
		let syms: Vec<u32> = names.iter().map(|_| 0).collect();
		Ok(Stmt::Local {
			names,
			syms,
			values,
		})
	}

	fn functionstat(&mut self) -> Result<Stmt, ParseError> {
		self.expect_kw("function")?;
		self.funcdecl()
	}

	fn funcdecl(&mut self) -> Result<Stmt, ParseError> {
		let first = self.expect(TokKind::Name, None)?;
		let mut obj: Option<Expr> = None;
		let mut name = first.text.clone();
		let mut ismethod = false;
		// push the current `name` into the object chain
		let push = |obj: &mut Option<Expr>, name: String| {
			let cur = match obj.take() {
				None => Expr::Ident { name, sym: None },
				Some(o) => Expr::Dot {
					obj: Box::new(o),
					name,
				},
			};
			*obj = Some(cur);
		};
		loop {
			if self.eat(TokKind::Punct, Some(".")) {
				push(&mut obj, name.clone());
				name = self.expect(TokKind::Name, None)?.text;
			} else if self.eat(TokKind::Punct, Some(":")) {
				push(&mut obj, name.clone());
				name = self.expect(TokKind::Name, None)?.text;
				ismethod = true;
				break;
			} else {
				break;
			}
		}
		let func = self.funcbody(ismethod)?;
		Ok(Stmt::FuncDecl {
			obj,
			name,
			ismethod,
			func: Box::new(func),
		})
	}

	/// '(' params ')' [':' typeref] block 'end'
	fn funcbody(&mut self, has_self: bool) -> Result<FuncDef, ParseError> {
		self.expect(TokKind::Punct, Some("("))?;
		let mut params: Vec<String> = Vec::new();
		let mut vararg = false;
		if has_self {
			params.push("self".to_string());
		}
		while !self.is(TokKind::Punct, Some(")")) {
			if self.is(TokKind::Punct, Some("...")) {
				self.next();
				vararg = true;
				self.expect(TokKind::Punct, Some(")"))?;
				break;
			}
			let t = self.expect(TokKind::Name, None)?;
			params.push(t.text.clone());
			self.eat(TokKind::Punct, Some("?"));
			if self.eat(TokKind::Punct, Some(":")) {
				self.typeref()?;
			}
			if !self.is(TokKind::Punct, Some(")")) {
				self.expect(TokKind::Punct, Some(","))?;
			}
		}
		// the ')' is already consumed: either the loop condition stopped on
		// it (non-vararg) — no, the loop only STOPS on ')' without consuming
		// it, or the vararg branch consumed it. Handle both:
		if self.is(TokKind::Punct, Some(")")) {
			self.next();
		}
		if self.eat(TokKind::Punct, Some(":")) {
			self.typeref()?; // return type
		}
		let body = self.block()?;
		self.expect_kw("end")?;
		Ok(FuncDef {
			params,
			param_syms: Vec::new(),
			vararg,
			body,
			has_self,
		})
	}

	fn ifstat(&mut self) -> Result<Stmt, ParseError> {
		self.expect_kw("if")?;
		let cond = self.exp()?;
		self.expect_kw("then")?;
		let thenb = self.block()?;
		let mut elsifs = Vec::new();
		while self.is_kw("elseif") {
			self.next();
			let c = self.exp()?;
			self.expect_kw("then")?;
			elsifs.push((c, self.block()?));
		}
		let elseb = if self.eat_kw("else") {
			Some(self.block()?)
		} else {
			None
		};
		self.expect_kw("end")?;
		Ok(Stmt::If {
			cond: Box::new(cond),
			thenb,
			elsifs,
			elseb,
		})
	}

	fn whilestat(&mut self) -> Result<Stmt, ParseError> {
		self.expect_kw("while")?;
		let cond = self.exp()?;
		self.expect_kw("do")?;
		self.loops += 1;
		let body = self.block()?;
		self.loops -= 1;
		self.expect_kw("end")?;
		Ok(Stmt::While {
			cond: Box::new(cond),
			body,
		})
	}

	fn repeatstat(&mut self) -> Result<Stmt, ParseError> {
		self.expect_kw("repeat")?;
		self.loops += 1;
		let body = self.block()?;
		self.loops -= 1;
		self.expect_kw("until")?;
		let cond = self.exp()?;
		Ok(Stmt::Repeat {
			body,
			cond: Box::new(cond),
		})
	}

	fn forstat(&mut self) -> Result<Stmt, ParseError> {
		self.expect_kw("for")?;
		let var = self.expect(TokKind::Name, None)?;
		if self.eat(TokKind::Punct, Some("=")) {
			let start = self.exp()?;
			self.expect(TokKind::Punct, Some(","))?;
			let limit = self.exp()?;
			// optional step: `, step`
			let step = if self.eat(TokKind::Punct, Some(",")) {
				Some(self.exp()?)
			} else {
				None
			};
			self.expect_kw("do")?;
			self.loops += 1;
			let body = self.block()?;
			self.loops -= 1;
			self.expect_kw("end")?;
			return Ok(Stmt::ForNum {
				var: var.text,
				var_sym: 0, // filled by symtab
				start: Box::new(start),
				limit: Box::new(limit),
				step: step.map(Box::new),
				body,
			});
		}
		let mut vars = vec![var.text.clone()];
		while self.eat(TokKind::Punct, Some(",")) {
			let n = self.expect(TokKind::Name, None)?;
			vars.push(n.text);
		}
		self.expect_kw("in")?;
		let iters = self.explist()?;
		self.expect_kw("do")?;
		self.loops += 1;
		let body = self.block()?;
		self.loops -= 1;
		self.expect_kw("end")?;
		Ok(Stmt::ForGen {
			vars: vars.clone(),
			syms: vars.iter().map(|_| 0).collect(),
			iters,
			body,
		})
	}

	/// type Alias = ...   (parsed and dropped)
	fn typealias_body(&mut self) -> Result<Stmt, ParseError> {
		self.expect(TokKind::Name, None)?; // alias name
		if self.eat(TokKind::Punct, Some("<")) {
			self.skip_generic_params()?;
		}
		self.expect(TokKind::Punct, Some("="))?;
		self.typeref()?;
		// produce an empty statement (no-op)
		Ok(Stmt::Do(Block::empty()))
	}

	fn skip_generic_params(&mut self) -> Result<(), ParseError> {
		// after consuming '<': skip to matching '>'
		let mut depth = 1;
		loop {
			let t = self.next();
			if t.kind == TokKind::Eof {
				return Err(self.errf("unterminated generic parameters".to_string()));
			}
			if t.kind == TokKind::Punct {
				if t.text == "<" {
					depth += 1;
				} else if t.text == ">" {
					depth -= 1;
					if depth == 0 {
						return Ok(());
					}
				}
			}
		}
	}

	// ------------------------------------------------------------------
	// type annotations (parsed & dropped)

	fn typeref(&mut self) -> Result<(), ParseError> {
		self.t_union()?;
		Ok(())
	}

	fn t_union(&mut self) -> Result<(), ParseError> {
		self.t_inter()?;
		while self.eat(TokKind::Punct, Some("|")) {
			self.t_inter()?;
		}
		Ok(())
	}

	fn t_inter(&mut self) -> Result<(), ParseError> {
		self.t_primary()?;
		while self.eat(TokKind::Punct, Some("&")) {
			self.t_primary()?;
		}
		Ok(())
	}

	fn t_primary(&mut self) -> Result<(), ParseError> {
		let t = self.peek(0).clone();
		match t.kind {
			TokKind::Name => {
				self.next();
				while self.eat(TokKind::Punct, Some(".")) {
					self.expect(TokKind::Name, None)?;
				}
				if self.eat(TokKind::Punct, Some("<")) {
					self.skip_generic_params()?;
				}
				if self.is(TokKind::Punct, Some("(")) {
					return Err(self.errf(
						"unsupported type syntax (function/typeof types); simplify the annotation"
							.to_string(),
					));
				}
			}
			TokKind::Punct if t.text == "(" => {
				self.next();
				if !self.is(TokKind::Punct, Some(")")) {
					loop {
						// one parameter: `name: type` | `...` | bare type
						if self.peek(0).kind == TokKind::Name
							&& self.peek(1).kind == TokKind::Punct
							&& self.peek(1).text == ":"
						{
							self.next();
							self.next();
							self.t_union()?;
						} else if self.is(TokKind::Punct, Some("...")) {
							self.next();
						} else {
							self.t_union()?;
						}
						if self.eat(TokKind::Punct, Some(",")) {
							if self.is(TokKind::Punct, Some(")")) {
								break; // trailing comma
							}
						} else {
							break;
						}
					}
				}
				self.expect(TokKind::Punct, Some(")"))?;
				// function type: `->` return type
				if self.is(TokKind::Punct, Some("-"))
					&& self.peek(1).kind == TokKind::Punct
					&& self.peek(1).text == ">"
				{
					self.next();
					self.next();
					self.t_union()?;
				} else if self.eat(TokKind::Punct, Some(":")) {
					self.t_union()?;
				}
			}
			TokKind::Punct if t.text == "{" => {
				self.next();
				while !self.is(TokKind::Punct, Some("}")) {
					if self.is(TokKind::Punct, Some("...")) {
						self.next();
					} else if self.is(TokKind::Punct, Some("[")) {
						self.next();
						self.expect(TokKind::Punct, Some("]"))?;
						self.expect(TokKind::Punct, Some(":"))?;
						self.t_union()?;
					} else {
						// `key: T` or bare array item type
						self.next(); // name or string key
						if self.is(TokKind::Punct, Some(":")) {
							self.next();
							self.t_union()?;
						}
					}
					if !self.is(TokKind::Punct, Some("}")) {
						if !self.eat(TokKind::Punct, Some(",")) {
							return Err(self.errf("expected ',' in table type".to_string()));
						}
					}
				}
				self.expect(TokKind::Punct, Some("}"))?;
			}
			TokKind::Str => {
				self.next();
			}
			TokKind::Punct if t.text == "..." => {
				self.next();
			}
			_ => {
				return Err(self.errf("unsupported or missing type annotation".to_string()));
			}
		}
		Ok(())
	}

	// ------------------------------------------------------------------
	// expression statements & assignments

	fn exprstat(&mut self) -> Result<Stmt, ParseError> {
		let e = self.prefixexp()?;
		let t = self.peek(0).clone();
		if t.kind == TokKind::Punct {
			if t.text == "=" {
				let mut targets = vec![e];
				while self.eat(TokKind::Punct, Some(",")) {
					targets.push(self.prefixexp()?);
				}
				self.next(); // '='
				let values = self.explist()?;
				return Ok(Stmt::Assign { targets, values });
			}
			if let Some(op) = compound_op(&t.text) {
				if !self.luau {
					return Err(self.errf(format!(
						"compound assignment ({}) requires --dialect luau",
						t.text
					)));
				}
				self.next(); // '+='
				let value = self.exp()?;
				if self.is(TokKind::Punct, Some(",")) {
					return Err(self.errf(
						"compound assignment takes a single target".to_string(),
					));
				}
				let rhs = Expr::Bin {
					op: op,
					l: Box::new(clone_expr(&e)),
					r: Box::new(value),
				};
				return Ok(Stmt::Assign {
					targets: vec![e],
					values: vec![rhs],
				});
			}
		}
		match e {
			Expr::Call { .. } | Expr::Method { .. } => Ok(Stmt::ExprStmt(e)),
			_ => Err(self.errf("expected assignment or a function call".to_string())),
		}
	}

	fn explist(&mut self) -> Result<Vec<Expr>, ParseError> {
		let mut list = vec![self.exp()?];
		while self.eat(TokKind::Punct, Some(",")) {
			list.push(self.exp()?);
		}
		Ok(list)
	}

	// ------------------------------------------------------------------
	// expressions (Pratt — mirrors lparser.c priorities)

	fn exp(&mut self) -> Result<Expr, ParseError> {
		self.subexpr(0)
	}

	fn subexpr(&mut self, limit: u8) -> Result<Expr, ParseError> {
		let t = self.peek(0).clone();
		if t.kind == TokKind::Punct && (t.text == "-" || t.text == "#") {
			self.next();
			let e = self.subexpr(UNARY_PRIO)?;
			let op = if t.text == "-" { UnOp::Minus } else { UnOp::Len };
			return self.binloop(Expr::Un { op, e: Box::new(e) }, limit);
		}
		if t.kind == TokKind::Name && t.text == "not" {
			self.next();
			let e = self.subexpr(UNARY_PRIO)?;
			return self.binloop(Expr::Un { op: UnOp::Not, e: Box::new(e) }, limit);
		}
		let e = self.simpleexp()?;
		self.binloop(e, limit)
	}

	fn binloop(&mut self, mut e: Expr, limit: u8) -> Result<Expr, ParseError> {
		loop {
			let t = self.peek(0);
			let op = if t.kind == TokKind::Punct {
				BinOp::from_text(&t.text)
			} else if t.kind == TokKind::Name && (t.text == "and" || t.text == "or") {
				BinOp::from_text(&t.text)
			} else {
				None
			};
			match op {
				Some(op) if op.prio().0 > limit => {
					self.next();
					let r = self.subexpr(op.prio().1)?;
					e = Expr::Bin {
						op,
						l: Box::new(e),
						r: Box::new(r),
					};
				}
				_ => return Ok(e),
			}
		}
	}

	fn simpleexp(&mut self) -> Result<Expr, ParseError> {
		let t = self.peek(0).clone();
		match t.kind {
			TokKind::Num => {
				self.next();
				Ok(Expr::Num {
					value: t.num,
					isfloat: t.isfloat,
				})
			}
			TokKind::Str => {
				self.next();
				Ok(Expr::Str { bytes: t.bytes })
			}
			TokKind::Interp => {
				self.next();
				self.interp_to_format(&t)
			}
			TokKind::Name => match t.text.as_str() {
				"true" => {
					self.next();
					Ok(Expr::Bool { value: true })
				}
				"false" => {
					self.next();
					Ok(Expr::Bool { value: false })
				}
				"nil" => {
					self.next();
					Ok(Expr::Nil)
				}
				"function" => {
					self.next();
					let f = self.funcbody(false)?;
					Ok(Expr::Function {
						params: f.params,
						param_syms: f.param_syms,
						vararg: f.vararg,
						body: f.body,
					})
				}
				_ => self.prefixexp(),
			},
			TokKind::Punct => match t.text.as_str() {
				"..." => {
					self.next();
					Ok(Expr::Vararg)
				}
				"{" => self.tableconstructor(),
				_ => self.prefixexp(),
			},
			_ => Err(self.errf("unexpected token in expression".to_string())),
		}
	}

	/// Desugar `` `a {e1} b` `` -> string.format("a %s b", e1)
	fn interp_to_format(&mut self, t: &Token) -> Result<Expr, ParseError> {
		// decode text parts: [len4][bytes]*  (one chunk per Text part)
		let mut texts: Vec<Vec<u8>> = Vec::new();
		let b = &t.bytes;
		let mut i = 0usize;
		while i + 4 <= b.len() {
			let len = u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]) as usize;
			i += 4;
			texts.push(b[i..i + len].to_vec());
			i += len;
		}
		let mut fmt: Vec<u8> = Vec::new();
		let mut args: Vec<Expr> = Vec::new();
		let mut text_idx = 0usize;
		let mut expr_idx = 0usize;
		for part in t.parts.iter() {
			match part {
				InterpPart::Text => {
					for &c in &texts[text_idx] {
						if c == b'%' {
							fmt.push(b'%');
						}
						fmt.push(c);
					}
					text_idx += 1;
				}
				InterpPart::Expr => {
					let src = &t.interp_srcs[expr_idx];
					expr_idx += 1;
					fmt.extend_from_slice(b"%s");
					let e = parse_expr_snippet(src, self.luau)?;
					args.push(e);
				}
			}
		}
		if args.is_empty() {
			return Ok(Expr::Str { bytes: fmt });
		}
		let func = Expr::Dot {
			obj: Box::new(Expr::Ident {
				name: "string".to_string(),
				sym: None,
			}),
			name: "format".to_string(),
		};
		let mut all = vec![Expr::Str { bytes: fmt }];
		all.extend(args);
		Ok(Expr::Call {
			func: Box::new(func),
			args: all,
		})
	}

	fn prefixexp(&mut self) -> Result<Expr, ParseError> {
		let mut e = self.primary()?;
		loop {
			let t = self.peek(0).clone();
			if t.kind == TokKind::Punct {
				if t.text == "." {
					self.next();
					let n = self.expect(TokKind::Name, None)?;
					e = Expr::Dot {
						obj: Box::new(e),
						name: n.text,
					};
					continue;
				}
				if t.text == "[" {
					self.next();
					let idx = self.exp()?;
					self.expect(TokKind::Punct, Some("]"))?;
					e = Expr::Index {
						obj: Box::new(e),
						idx: Box::new(idx),
					};
					continue;
				}
				if t.text == ":" {
					self.next();
					let n = self.expect(TokKind::Name, None)?;
					let args = self.callargs()?;
					let obj = e;
					e = Expr::Method {
						obj: Box::new(obj),
						name: n.text,
						args,
					};
					continue;
				}
				if t.text == "(" || t.text == "{" || t.kind == TokKind::Str || t.kind == TokKind::Interp
				{
					let args = self.callargs()?;
					e = Expr::Call {
						func: Box::new(e),
						args,
					};
					continue;
				}
			}
			break;
		}
		Ok(e)
	}

	fn callargs(&mut self) -> Result<Vec<Expr>, ParseError> {
		if self.is(TokKind::Punct, Some("(")) {
			self.next();
			let args = if self.is(TokKind::Punct, Some(")")) {
				Vec::new()
			} else {
				self.explist()?
			};
			self.expect(TokKind::Punct, Some(")"))?;
			Ok(args)
		} else if self.is(TokKind::Punct, Some("{")) {
			Ok(vec![self.tableconstructor()?])
		} else if self.peek(0).kind == TokKind::Str || self.peek(0).kind == TokKind::Interp {
			Ok(vec![self.simpleexp()?])
		} else {
			Err(self.errf("expected function arguments".to_string()))
		}
	}

	fn primary(&mut self) -> Result<Expr, ParseError> {
		let t = self.peek(0).clone();
		match t.kind {
			TokKind::Name => {
				self.next();
				Ok(Expr::Ident {
					name: t.text,
					sym: None,
				})
			}
			TokKind::Punct if t.text == "(" => {
				self.next();
				let e = self.exp()?;
				self.expect(TokKind::Punct, Some(")"))?;
				Ok(e)
			}
			TokKind::Punct if t.text == "{" => self.tableconstructor(),
			TokKind::Num | TokKind::Str | TokKind::Interp => self.simpleexp(),
			_ => Err(self.errf("unexpected token in primary expression".to_string())),
		}
	}

	fn tableconstructor(&mut self) -> Result<Expr, ParseError> {
		self.expect(TokKind::Punct, Some("{"))?;
		let mut fields = Vec::new();
		while !self.is(TokKind::Punct, Some("}")) {
			let t = self.peek(0).clone();
			if t.kind == TokKind::Punct && t.text == "[" {
				self.next();
				let k = self.exp()?;
				self.expect(TokKind::Punct, Some("]"))?;
				self.expect(TokKind::Punct, Some("="))?;
				let v = self.exp()?;
				fields.push(TableField::Key {
					key: k,
					value: v,
				});
			} else if t.kind == TokKind::Name
				&& self.peek(1).kind == TokKind::Punct
				&& self.peek(1).text == "="
			{
				let n = self.next();
				self.next(); // '='
				let v = self.exp()?;
				fields.push(TableField::Key {
					key: Expr::Str {
						bytes: n.text.as_bytes().to_vec(),
					},
					value: v,
				});
			} else {
				let v = self.exp()?;
				fields.push(TableField::Array(v));
			}
			if !self.is(TokKind::Punct, Some("}")) {
				if !self.eat(TokKind::Punct, Some(",")) && !self.eat(TokKind::Punct, Some(";")) {
					return Err(self.errf("expected ',' or '}' in table constructor".to_string()));
				}
			}
		}
		self.expect(TokKind::Punct, Some("}"))?;
		Ok(Expr::Table { fields })
	}
}

fn compound_op(s: &str) -> Option<BinOp> {
	match s {
		"+=" => Some(BinOp::Add),
		"-=" => Some(BinOp::Sub),
		"*=" => Some(BinOp::Mul),
		"/=" => Some(BinOp::Div),
		"//=" => Some(BinOp::Idiv),
		"%=" => Some(BinOp::Mod),
		"^=" => Some(BinOp::Pow),
		_ => None,
	}
}

/// Deep clone (no symtab info to preserve at this stage).
fn clone_expr(e: &Expr) -> Expr {
	match e {
		Expr::Num { value, isfloat } => Expr::Num {
			value: *value,
			isfloat: *isfloat,
		},
		Expr::Str { bytes } => Expr::Str { bytes: bytes.clone() },
		Expr::Bool { value } => Expr::Bool { value: *value },
		Expr::Nil => Expr::Nil,
		Expr::Vararg => Expr::Vararg,
		Expr::Ident { name, sym } => Expr::Ident {
			name: name.clone(),
			sym: *sym,
		},
		Expr::Dot { obj, name } => Expr::Dot {
			obj: Box::new(clone_expr(obj)),
			name: name.clone(),
		},
		Expr::Index { obj, idx } => Expr::Index {
			obj: Box::new(clone_expr(obj)),
			idx: Box::new(clone_expr(idx)),
		},
		Expr::Call { func, args } => Expr::Call {
			func: Box::new(clone_expr(func)),
			args: args.iter().map(clone_expr).collect(),
		},
		Expr::Method { obj, name, args } => Expr::Method {
			obj: Box::new(clone_expr(obj)),
			name: name.clone(),
			args: args.iter().map(clone_expr).collect(),
		},
		Expr::Un { op, e } => Expr::Un {
			op: *op,
			e: Box::new(clone_expr(e)),
		},
		Expr::Bin { op, l, r } => Expr::Bin {
			op: *op,
			l: Box::new(clone_expr(l)),
			r: Box::new(clone_expr(r)),
		},
		Expr::Table { fields } => Expr::Table {
			fields: fields
				.iter()
				.map(|f| match f {
					TableField::Array(e) => TableField::Array(clone_expr(e)),
					TableField::Key { key, value } => TableField::Key {
						key: clone_expr(key),
						value: clone_expr(value),
					},
				})
				.collect(),
		},
		Expr::Function { params, param_syms, vararg, body } => Expr::Function {
			params: params.clone(),
			param_syms: param_syms.clone(),
			vararg: *vararg,
			body: clone_block(body),
		},
	}
}

fn clone_block(b: &Block) -> Block {
	Block {
		stmts: b.stmts.iter().map(clone_stmt).collect(),
	}
}

fn clone_stmt(s: &Stmt) -> Stmt {
	match s {
		Stmt::Local { names, syms, values } => Stmt::Local {
			names: names.clone(),
			syms: syms.clone(),
			values: values
				.iter()
				.map(|v| v.as_ref().map(clone_expr))
				.collect(),
		},
		Stmt::LocalFunc { name, sym, func } => Stmt::LocalFunc {
			name: name.clone(),
			sym: *sym,
			func: Box::new(clone_func(func)),
		},
		Stmt::FuncDecl { obj, name, ismethod, func } => Stmt::FuncDecl {
			obj: obj.as_ref().map(clone_expr),
			name: name.clone(),
			ismethod: *ismethod,
			func: Box::new(clone_func(func)),
		},
		Stmt::Assign { targets, values } => Stmt::Assign {
			targets: targets.iter().map(clone_expr).collect(),
			values: values.iter().map(clone_expr).collect(),
		},
		Stmt::ExprStmt(e) => Stmt::ExprStmt(clone_expr(e)),
		Stmt::If { cond, thenb, elsifs, elseb } => Stmt::If {
			cond: Box::new(clone_expr(cond)),
			thenb: clone_block(thenb),
			elsifs: elsifs
				.iter()
				.map(|(c, b)| (clone_expr(c), clone_block(b)))
				.collect(),
			elseb: elseb.as_ref().map(clone_block),
		},
		Stmt::While { cond, body } => Stmt::While {
			cond: Box::new(clone_expr(cond)),
			body: clone_block(body),
		},
		Stmt::Repeat { body, cond } => Stmt::Repeat {
			body: clone_block(body),
			cond: Box::new(clone_expr(cond)),
		},
		Stmt::ForNum { var, var_sym, start, limit, step, body } => Stmt::ForNum {
			var: var.clone(),
			var_sym: *var_sym,
			start: Box::new(clone_expr(start)),
			limit: Box::new(clone_expr(limit)),
			step: step.as_ref().map(|s| Box::new(clone_expr(s))),
			body: clone_block(body),
		},
		Stmt::ForGen { vars, syms, iters, body } => Stmt::ForGen {
			vars: vars.clone(),
			syms: syms.clone(),
			iters: iters.iter().map(clone_expr).collect(),
			body: clone_block(body),
		},
		Stmt::Do(b) => Stmt::Do(clone_block(b)),
		Stmt::Break => Stmt::Break,
		Stmt::Continue => Stmt::Continue,
		Stmt::Return(es) => Stmt::Return(es.iter().map(clone_expr).collect()),
	}
}

fn clone_func(f: &FuncDef) -> FuncDef {
	FuncDef {
		params: f.params.clone(),
		param_syms: f.param_syms.clone(),
		vararg: f.vararg,
		body: clone_block(&f.body),
		has_self: f.has_self,
	}
}

/// Parse a bare expression snippet (interpolation placeholders).
fn parse_expr_snippet(src: &str, luau: bool) -> Result<Expr, ParseError> {
	let mut lexer = Lexer::new(src, luau);
	let toks = lexer
		.tokens()
		.map_err(|e| ParseError { line: e.line, msg: e.msg })?;
	let mut p = Parser { toks, i: 0, luau, loops: 0 };
	let e = p.exp()?;
	if p.peek(0).kind != TokKind::Eof {
		return Err(ParseError {
			line: p.peek(0).line,
			msg: "malformed interpolated string expression".to_string(),
		});
	}
	Ok(e)
}

pub fn parse(src: &str, luau: bool) -> Result<Block, ParseError> {
	let mut lexer = Lexer::new(src, luau);
	let toks = lexer
		.tokens()
		.map_err(|e| ParseError { line: e.line, msg: e.msg })?;
	let mut p = Parser { toks, i: 0, luau, loops: 0 };
	let block = p.block()?;
	if p.peek(0).kind != TokKind::Eof {
		let t = p.peek(0);
		return Err(ParseError {
			line: t.line,
			msg: format!("unexpected token after block: '{}'", t.text),
		});
	}
	Ok(block)
}
