//! AST definitions for Lua 5.1 / Luau source.
//!
//! Conventions:
//! - Strings are byte strings (Vec<u8>) — Lua strings are arbitrary bytes.
//! - Identifiers carry an optional SymId, filled in by the symtab resolver.
//! - Method calls are kept as their own node (obj, name, args) — `self` is NOT
//!   added to args; semantics = obj.name(obj, args...).
//! - Backtick interpolation (Luau) is desugared to string.format at parse time
//!   (semantically identical in both dialects).

use std::fmt;

pub type SymId = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
	Add,
	Sub,
	Mul,
	Div,
	/// Luau `//` — stored as its own op; the printer emits math.floor(l / r)
	/// so the output is valid in both dialects
	Idiv,
	Mod,
	Pow,
	Concat,
	And,
	Or,
	Eq,
	Ne,
	Lt,
	Gt,
	Le,
	Ge,
}

impl BinOp {
	pub fn from_text(s: &str) -> Option<BinOp> {
		match s {
			"+" => Some(BinOp::Add),
			"-" => Some(BinOp::Sub),
			"*" => Some(BinOp::Mul),
			"/" => Some(BinOp::Div),
			"//" => Some(BinOp::Idiv),
			"%" => Some(BinOp::Mod),
			"^" => Some(BinOp::Pow),
			".." => Some(BinOp::Concat),
			"and" => Some(BinOp::And),
			"or" => Some(BinOp::Or),
			"==" => Some(BinOp::Eq),
			"~=" => Some(BinOp::Ne),
			"<" => Some(BinOp::Lt),
			">" => Some(BinOp::Gt),
			"<=" => Some(BinOp::Le),
			">=" => Some(BinOp::Ge),
			_ => None,
		}
	}

	pub fn text(&self) -> &'static str {
		match self {
			BinOp::Add => "+",
			BinOp::Sub => "-",
			BinOp::Mul => "*",
			BinOp::Div => "/",
			BinOp::Idiv => "//",
			BinOp::Mod => "%",
			BinOp::Pow => "^",
			BinOp::Concat => "..",
			BinOp::And => "and",
			BinOp::Or => "or",
			BinOp::Eq => "==",
			BinOp::Ne => "~=",
			BinOp::Lt => "<",
			BinOp::Gt => ">",
			BinOp::Le => "<=",
			BinOp::Ge => ">=",
		}
	}

	/// (left, right) Pratt priorities — identical to Lua 5.1 lparser.c and
	/// Luau Parser.cpp (// sits at the mul level in Luau; we desugar it).
	pub fn prio(&self) -> (u8, u8) {
		match self {
			BinOp::Or => (1, 1),
			BinOp::And => (2, 2),
			BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => (3, 3),
			BinOp::Concat => (5, 4),
			BinOp::Add | BinOp::Sub => (6, 6),
			BinOp::Mul | BinOp::Div | BinOp::Idiv | BinOp::Mod => (7, 7),
			BinOp::Pow => (10, 9),
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
	Not,
	Minus,
	Len,
}

impl UnOp {
	pub fn text(&self) -> &'static str {
		match self {
			UnOp::Not => "not",
			UnOp::Minus => "-",
			UnOp::Len => "#",
		}
	}
}

#[derive(Debug, Clone)]
pub enum Expr {
	Num { value: f64, isfloat: bool },
	/// `is_binary`: ciphertext/key-stream/bytecode blobs (printer escapes
	/// every non-printable-ASCII byte — no UTF-8 passthrough, which would
	/// otherwise emit random CJK garbage from arbitrary ciphertext bytes)
	Str { bytes: Vec<u8>, is_binary: bool },
	Bool { value: bool },
	Nil,
	Vararg,
	/// name = source name; sym = None for globals (filled by symtab)
	Ident { name: String, sym: Option<SymId> },
	Dot { obj: Box<Expr>, name: String },
	Index { obj: Box<Expr>, idx: Box<Expr> },
	Call { func: Box<Expr>, args: Vec<Expr> },
	Method { obj: Box<Expr>, name: String, args: Vec<Expr> },
	Un { op: UnOp, e: Box<Expr> },
	Bin { op: BinOp, l: Box<Expr>, r: Box<Expr> },
	Table { fields: Vec<TableField> },
	Function {
		params: Vec<String>,
		/// SymId per parameter (filled by symtab; printer uses it so that
		/// mangling renames the declaration, not just body references)
		param_syms: Vec<SymId>,
		vararg: bool,
		body: Block,
	},
}

#[derive(Debug, Clone)]
pub enum TableField {
	Array(Expr),
	Key { key: Expr, value: Expr },
}

#[derive(Debug, Clone)]
pub struct Block {
	pub stmts: Vec<Stmt>,
}

impl Block {
	pub fn empty() -> Block {
		Block { stmts: Vec::new() }
	}
}

#[derive(Debug, Clone)]
pub enum Stmt {
	/// names[i] is the source name; syms[i] filled by symtab;
	/// values[i] = None means no initializer
	Local {
		names: Vec<String>,
		syms: Vec<SymId>,
		values: Vec<Option<Expr>>,
	},
	LocalFunc { name: String, sym: SymId, func: Box<FuncDef> },
	FuncDecl {
		/// dotted object chain (None for bare `function f()`); the LAST name
		/// (or method name) is in `name`; obj is the expr before it
		obj: Option<Expr>,
		name: String,
		ismethod: bool,
		func: Box<FuncDef>,
	},
	Assign { targets: Vec<Expr>, values: Vec<Expr> },
	ExprStmt(Expr),
	If {
		cond: Box<Expr>,
		thenb: Block,
		elsifs: Vec<(Expr, Block)>,
		elseb: Option<Block>,
	},
	While { cond: Box<Expr>, body: Block },
	Repeat { body: Block, cond: Box<Expr> },
	ForNum {
		var: String,
		var_sym: SymId,
		start: Box<Expr>,
		limit: Box<Expr>,
		step: Option<Box<Expr>>,
		body: Block,
	},
	ForGen { vars: Vec<String>, syms: Vec<SymId>, iters: Vec<Expr>, body: Block },
	Do(Block),
	Break,
	Continue,
	Return(Vec<Expr>),
}

#[derive(Debug, Clone)]
pub struct FuncDef {
	pub params: Vec<String>,
	/// SymId per parameter (filled by symtab; used by the printer so that
	/// mangling renames the declaration, not just the body references)
	pub param_syms: Vec<SymId>,
	pub vararg: bool,
	pub body: Block,
	/// true when this is a method declaration (params[0] == "self" implicit)
	pub has_self: bool,
}

#[derive(Debug)]
pub struct ParseError {
	pub line: usize,
	pub msg: String,
}

impl fmt::Display for ParseError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "parse error line {}: {}", self.line, self.msg)
	}
}

impl std::error::Error for ParseError {}
