//! L2 string encryption.
//!
//! Every string literal in the program is replaced by a call to a runtime
//! decoder (a chunk-level local, visible to all nested functions as an
//! upvalue). Cipher: additive keystream (5.1-safe — no bitops needed, and
//! Lua 5.1 and Luau share floor `%` semantics — verified):
//!
//!     enc[i] = (byte[i] + key[i mod k] + i) mod 256      (i is 1-based)
//!     dec[i] = (byte[i] - key[i mod k] - i) mod 256
//!
//! The ciphertext is split into 3 chunks passed as separate arguments
//! (unrolling). The key is embedded as three split string literals and
//! expanded into a byte table at runtime — no contiguous key literal.

use crate::ast::*;
use crate::mangle::{gen_name, reserved_set};
use crate::rng::Rng;
use crate::symtab::{Sym, SymTable};
use std::collections::HashSet;

pub(crate) const KEY_LEN: usize = 24;
const PARTS: usize = 3;

struct Cfg {
	key: Vec<u8>,
	dec: SymId,
	dec_name: String,
}

pub(crate) fn encrypt(bytes: &[u8], key: &[u8]) -> Vec<u8> {
	let k = key.len();
	bytes
		.iter()
		.enumerate()
		.map(|(i, &b)| (b as u32 + key[i % k] as u32 + (i + 1) as u32) % 256)
		.map(|v| v as u8)
		.collect()
}

pub(crate) fn split_into_parts(bytes: &[u8], parts: usize) -> Vec<Vec<u8>> {
	if bytes.is_empty() {
		return vec![Vec::new()];
	}
	let n = parts.min(bytes.len().max(1));
	let base = bytes.len() / n;
	let rem = bytes.len() % n;
	let mut out = Vec::new();
	let mut start = 0;
	for i in 0..n {
		let len = base + if i < rem { 1 } else { 0 };
		out.push(bytes[start..start + len].to_vec());
		start += len;
	}
	out
}

fn ident(name: &str, sym: Option<SymId>) -> Expr {
	Expr::Ident {
		name: name.to_string(),
		sym,
	}
}

fn num(v: f64) -> Expr {
	Expr::Num {
		value: v,
		isfloat: false,
	}
}

fn global_call(table_name: &str, fn_name: &str, args: Vec<Expr>) -> Expr {
	Expr::Call {
		func: Box::new(Expr::Dot {
			obj: Box::new(ident(table_name, None)),
			name: fn_name.to_string(),
		}),
		args,
	}
}

fn contains_str(b: &Block) -> bool {
	b.stmts.iter().any(|s| stmt_has_str(s))
}

fn stmt_has_str(s: &Stmt) -> bool {
	match s {
		Stmt::Local { values, .. } => values.iter().any(|v| v.as_ref().map_or(false, expr_has_str)),
		Stmt::LocalFunc { func, .. } | Stmt::FuncDecl { func, .. } => {
			func.body.stmts.iter().any(stmt_has_str)
		}
		Stmt::Assign { targets, values } => {
			targets.iter().any(expr_has_str) || values.iter().any(expr_has_str)
		}
		Stmt::ExprStmt(e) => expr_has_str(e),
		Stmt::If { cond, thenb, elsifs, elseb } => {
			expr_has_str(cond)
				|| thenb.stmts.iter().any(stmt_has_str)
				|| elsifs.iter().any(|(c, b)| expr_has_str(c) || b.stmts.iter().any(stmt_has_str))
				|| elseb
					.as_ref()
					.map_or(false, |b| b.stmts.iter().any(stmt_has_str))
		}
		Stmt::While { cond, body } => expr_has_str(cond) || body.stmts.iter().any(stmt_has_str),
		Stmt::Repeat { body, cond } => body.stmts.iter().any(stmt_has_str) || expr_has_str(cond),
		Stmt::ForNum {
			start, limit, step, body, ..
		} => {
			expr_has_str(start)
				|| expr_has_str(limit)
				|| step.as_ref().map_or(false, |s| expr_has_str(s))
				|| body.stmts.iter().any(stmt_has_str)
		}
		Stmt::ForGen { iters, body, .. } => {
			iters.iter().any(expr_has_str) || body.stmts.iter().any(stmt_has_str)
		}
		Stmt::Do(b) => b.stmts.iter().any(stmt_has_str),
		Stmt::Return(es) => es.iter().any(expr_has_str),
		_ => false,
	}
}

fn expr_has_str(e: &Expr) -> bool {
	match e {
		Expr::Str { .. } => true,
		Expr::Dot { obj, .. } => expr_has_str(obj),
		Expr::Index { obj, idx } => expr_has_str(obj) || expr_has_str(idx),
		Expr::Call { func, args } => expr_has_str(func) || args.iter().any(expr_has_str),
		Expr::Method { obj, args, .. } => expr_has_str(obj) || args.iter().any(expr_has_str),
		Expr::Un { e, .. } => expr_has_str(e),
		Expr::Bin { l, r, .. } => expr_has_str(l) || expr_has_str(r),
		Expr::Table { fields } => fields.iter().any(|f| match f {
			TableField::Array(e) => expr_has_str(e),
			TableField::Key { key, value } => expr_has_str(key) || expr_has_str(value),
		}),
		Expr::Function { body, .. } => body.stmts.iter().any(stmt_has_str),
		_ => false,
	}
}

/// Replace Str nodes with decoder calls. One pass — the Str arm builds fresh
/// chunk Str nodes that are never revisited.
fn xform_expr(e: &Expr, cfg: &Cfg) -> Expr {
	match e {
		Expr::Str { bytes, .. } => {
			let ct = encrypt(bytes, &cfg.key);
			let chunks = split_into_parts(&ct, PARTS);
			let args: Vec<Expr> = chunks
				.into_iter()
				.map(|c| Expr::Str { bytes: c, is_binary: true })
				.collect();
			Expr::Call {
				func: Box::new(ident(&cfg.dec_name, Some(cfg.dec))),
				args,
			}
		}
		Expr::Dot { obj, name } => Expr::Dot {
			obj: Box::new(xform_expr(obj, cfg)),
			name: name.clone(),
		},
		Expr::Index { obj, idx } => Expr::Index {
			obj: Box::new(xform_expr(obj, cfg)),
			idx: Box::new(xform_expr(idx, cfg)),
		},
		Expr::Call { func, args } => Expr::Call {
			func: Box::new(xform_expr(func, cfg)),
			args: args.iter().map(|a| xform_expr(a, cfg)).collect(),
		},
		Expr::Method { obj, name, args } => Expr::Method {
			obj: Box::new(xform_expr(obj, cfg)),
			name: name.clone(),
			args: args.iter().map(|a| xform_expr(a, cfg)).collect(),
		},
		Expr::Un { op, e } => Expr::Un {
			op: *op,
			e: Box::new(xform_expr(e, cfg)),
		},
		Expr::Bin { op, l, r } => Expr::Bin {
			op: *op,
			l: Box::new(xform_expr(l, cfg)),
			r: Box::new(xform_expr(r, cfg)),
		},
		Expr::Table { fields } => Expr::Table {
			fields: fields
				.iter()
				.map(|f| match f {
					TableField::Array(e) => TableField::Array(xform_expr(e, cfg)),
					TableField::Key { key, value } => TableField::Key {
						key: xform_expr(key, cfg),
						value: xform_expr(value, cfg),
					},
				})
				.collect(),
		},
		Expr::Function { params, param_syms, vararg, body } => Expr::Function {
			params: params.clone(),
			param_syms: param_syms.clone(),
			vararg: *vararg,
			body: xform_block(body, cfg),
		},
		other => other.clone(),
	}
}

fn xform_stmt(s: &Stmt, cfg: &Cfg) -> Stmt {
	match s {
		Stmt::Local { names, syms, values } => Stmt::Local {
			names: names.clone(),
			syms: syms.clone(),
			values: values
				.iter()
				.map(|v| v.as_ref().map(|e| xform_expr(e, cfg)))
				.collect(),
		},
		Stmt::LocalFunc { name, sym, func } => Stmt::LocalFunc {
			name: name.clone(),
			sym: *sym,
			func: Box::new(FuncDef {
				params: func.params.clone(),
				param_syms: func.param_syms.clone(),
				vararg: func.vararg,
				body: xform_block(&func.body, cfg),
				has_self: func.has_self,
			}),
		},
		Stmt::FuncDecl { obj, name, ismethod, func } => Stmt::FuncDecl {
			obj: obj.as_ref().map(|o| xform_expr(o, cfg)),
			name: name.clone(),
			ismethod: *ismethod,
			func: Box::new(FuncDef {
				params: func.params.clone(),
				param_syms: func.param_syms.clone(),
				vararg: func.vararg,
				body: xform_block(&func.body, cfg),
				has_self: func.has_self,
			}),
		},
		Stmt::Assign { targets, values } => Stmt::Assign {
			targets: targets.iter().map(|t| xform_expr(t, cfg)).collect(),
			values: values.iter().map(|v| xform_expr(v, cfg)).collect(),
		},
		Stmt::ExprStmt(e) => Stmt::ExprStmt(xform_expr(e, cfg)),
		Stmt::If { cond, thenb, elsifs, elseb } => Stmt::If {
			cond: Box::new(xform_expr(cond, cfg)),
			thenb: xform_block(thenb, cfg),
			elsifs: elsifs
				.iter()
				.map(|(c, b)| (xform_expr(c, cfg), xform_block(b, cfg)))
				.collect(),
			elseb: elseb.as_ref().map(|b| xform_block(b, cfg)),
		},
		Stmt::While { cond, body } => Stmt::While {
			cond: Box::new(xform_expr(cond, cfg)),
			body: xform_block(body, cfg),
		},
		Stmt::Repeat { body, cond } => Stmt::Repeat {
			body: xform_block(body, cfg),
			cond: Box::new(xform_expr(cond, cfg)),
		},
		Stmt::ForNum {
			var,
			var_sym,
			start,
			limit,
			step,
			body,
		} => Stmt::ForNum {
			var: var.clone(),
			var_sym: *var_sym,
			start: Box::new(xform_expr(start, cfg)),
			limit: Box::new(xform_expr(limit, cfg)),
			step: step.as_ref().map(|s| Box::new(xform_expr(s, cfg))),
			body: xform_block(body, cfg),
		},
		Stmt::ForGen { vars, syms, iters, body } => Stmt::ForGen {
			vars: vars.clone(),
			syms: syms.clone(),
			iters: iters.iter().map(|i| xform_expr(i, cfg)).collect(),
			body: xform_block(body, cfg),
		},
		Stmt::Do(b) => Stmt::Do(xform_block(b, cfg)),
		Stmt::Return(es) => Stmt::Return(es.iter().map(|e| xform_expr(e, cfg)).collect()),
		other => other.clone(),
	}
}

fn xform_block(b: &Block, cfg: &Cfg) -> Block {
	Block {
		stmts: b.stmts.iter().map(|s| xform_stmt(s, cfg)).collect(),
	}
}

/// Build the runtime loader. All loader locals get real SymIds (created in
/// `table`) so the printer emits them as locals, not globals:
///
///   local K1, K2, K3 = "<chunk1>", "<chunk2>", "<chunk3>"
///   local KC = K1 .. K2 .. K3
///   local KT = {}
///   for I = 1, #KC do KT[I] = string.byte(KC, I) end
///   local function DEC(...)
///     local P = { ... }
///     local S = table.concat(P)
///     local O = {}
///     for I = 1, #S do
///       local K = KT[((I - 1) % #KT) + 1]
///       local D = string.byte(S, I) - K - I
///       local R = D % 256
///       if R < 0 then R = R + 256 end
///       O[I] = string.char(R)
///     end
///     return table.concat(O)
///   end
pub(crate) fn build_loader(
	table: &mut SymTable,
	rng: &mut Rng,
	reserved: &mut HashSet<String>,
	key: &[u8],
) -> (Vec<Stmt>, SymId, String) {
	let new_sym = |table: &mut SymTable, name: String| -> SymId {
		let id = table.syms.len() as SymId;
		table.syms.push(Sym {
			name,
			is_param: false,
			keep_name: false,
		});
		id
	};
	let next_name = |reserved: &mut HashSet<String>, rng: &mut Rng| -> String {
		loop {
			let n = gen_name(rng, reserved);
			reserved.insert(n.clone());
			return n;
		}
	};

	let n1 = next_name(reserved, rng);
	let n2 = next_name(reserved, rng);
	let n3 = next_name(reserved, rng);
	let nk = next_name(reserved, rng);
	let kt = next_name(reserved, rng);
	let ni2 = next_name(reserved, rng);
	let dec_name = next_name(reserved, rng);
	let np = next_name(reserved, rng);
	let ns = next_name(reserved, rng);
	let no = next_name(reserved, rng);
	let ni = next_name(reserved, rng);
	let nk2 = next_name(reserved, rng);
	let nd = next_name(reserved, rng);
	let nr = next_name(reserved, rng);

	let s1 = new_sym(table, n1.clone());
	let s2 = new_sym(table, n2.clone());
	let s3 = new_sym(table, n3.clone());
	let sk = new_sym(table, nk.clone());
	let skt = new_sym(table, kt.clone());
	let si2 = new_sym(table, ni2.clone());
	let sdec = new_sym(table, dec_name.clone());
	let sp = new_sym(table, np.clone());
	let ss = new_sym(table, ns.clone());
	let so = new_sym(table, no.clone());
	let si = new_sym(table, ni.clone());
	let sk2 = new_sym(table, nk2.clone());
	let sd = new_sym(table, nd.clone());
	let sr = new_sym(table, nr.clone());

	let i = ident(&ni, Some(si));
	let k = ident(&nk2, Some(sk2));
	let d = ident(&nd, Some(sd));
	let r = ident(&nr, Some(sr));
	let kt_id = ident(&kt, Some(skt));
	let p = ident(&np, Some(sp));
	let s = ident(&ns, Some(ss));
	let o = ident(&no, Some(so));

	let loop_body = Block {
		stmts: vec![
			Stmt::Local {
				names: vec![nk2.clone()],
				syms: vec![sk2],
				values: vec![Some(Expr::Index {
					obj: Box::new(kt_id.clone()),
					idx: Box::new(Expr::Bin {
						op: BinOp::Add,
						l: Box::new(Expr::Bin {
							op: BinOp::Mod,
							l: Box::new(Expr::Bin {
								op: BinOp::Sub,
								l: Box::new(i.clone()),
								r: Box::new(num(1.0)),
							}),
							r: Box::new(Expr::Un {
								op: UnOp::Len,
								e: Box::new(kt_id.clone()),
							}),
						}),
						r: Box::new(num(1.0)),
					}),
				})],
			},
			Stmt::Local {
				names: vec![nd.clone()],
				syms: vec![sd],
				values: vec![Some(Expr::Bin {
					op: BinOp::Sub,
					l: Box::new(Expr::Bin {
						op: BinOp::Sub,
						l: Box::new(global_call(
							"string",
							"byte",
							vec![s.clone(), i.clone()],
						)),
						r: Box::new(k.clone()),
					}),
					r: Box::new(i.clone()),
				})],
			},
			Stmt::Local {
				names: vec![nr.clone()],
				syms: vec![sr],
				values: vec![Some(Expr::Bin {
					op: BinOp::Mod,
					l: Box::new(d.clone()),
					r: Box::new(num(256.0)),
				})],
			},
			Stmt::If {
				cond: Box::new(Expr::Bin {
					op: BinOp::Lt,
					l: Box::new(r.clone()),
					r: Box::new(num(0.0)),
				}),
				thenb: Block {
					stmts: vec![Stmt::Assign {
						targets: vec![r.clone()],
						values: vec![Expr::Bin {
							op: BinOp::Add,
							l: Box::new(r.clone()),
							r: Box::new(num(256.0)),
						}],
					}],
				},
				elsifs: vec![],
				elseb: None,
			},
			Stmt::Assign {
				targets: vec![Expr::Index {
					obj: Box::new(o.clone()),
					idx: Box::new(i.clone()),
				}],
				values: vec![global_call("string", "char", vec![r.clone()])],
			},
		],
	};
	let decoder_body = Block {
		stmts: vec![
			Stmt::Local {
				names: vec![np.clone()],
				syms: vec![sp],
				values: vec![Some(Expr::Table {
					fields: vec![TableField::Array(Expr::Vararg)],
				})],
			},
			Stmt::Local {
				names: vec![ns.clone()],
				syms: vec![ss],
				values: vec![Some(global_call("table", "concat", vec![p]))],
			},
			Stmt::Local {
				names: vec![no.clone()],
				syms: vec![so],
				values: vec![Some(Expr::Table { fields: vec![] })],
			},
			Stmt::ForNum {
				var: ni.clone(),
				var_sym: si,
				start: Box::new(num(1.0)),
				limit: Box::new(Expr::Un {
					op: UnOp::Len,
					e: Box::new(s),
				}),
				step: None,
				body: loop_body,
			},
			Stmt::Return(vec![global_call("table", "concat", vec![o])]),
		],
	};

	let k1 = key[0..KEY_LEN / 3].to_vec();
	let k2 = key[KEY_LEN / 3..KEY_LEN * 2 / 3].to_vec();
	let k3 = key[KEY_LEN * 2 / 3..KEY_LEN].to_vec();

	let loader = vec![
		Stmt::Local {
			names: vec![n1, n2, n3],
			syms: vec![s1, s2, s3],
			values: vec![
				Some(Expr::Str { bytes: k1, is_binary: true }),
				Some(Expr::Str { bytes: k2, is_binary: true }),
				Some(Expr::Str { bytes: k3, is_binary: true }),
			],
		},
		Stmt::Local {
			names: vec![nk],
			syms: vec![sk],
			values: vec![Some(Expr::Bin {
				op: BinOp::Concat,
				l: Box::new(ident("", Some(s1))),
				r: Box::new(Expr::Bin {
					op: BinOp::Concat,
					l: Box::new(ident("", Some(s2))),
					r: Box::new(ident("", Some(s3))),
				}),
			})],
		},
		Stmt::Local {
			names: vec![kt],
			syms: vec![skt],
			values: vec![Some(Expr::Table { fields: vec![] })],
		},
		Stmt::ForNum {
			var: ni2.clone(),
			var_sym: si2,
			start: Box::new(num(1.0)),
			limit: Box::new(Expr::Un {
				op: UnOp::Len,
				e: Box::new(ident("", Some(sk))),
			}),
			step: None,
			body: Block {
				stmts: vec![Stmt::Assign {
					targets: vec![Expr::Index {
						obj: Box::new(ident("", Some(skt))),
						idx: Box::new(ident("", Some(si2))),
					}],
					values: vec![global_call(
						"string",
						"byte",
						vec![ident("", Some(sk)), ident("", Some(si2))],
					)],
				}],
			},
		},
		Stmt::LocalFunc {
			name: dec_name.clone(),
			sym: sdec,
			func: Box::new(FuncDef {
				params: vec![],
				param_syms: vec![],
				vararg: true,
				body: decoder_body,
				has_self: false,
			}),
		},
	];
	(loader, sdec, dec_name)
}

pub fn apply_strings(
	block: &mut Block,
	table: &mut SymTable,
	rng: &mut Rng,
	extra_reserved: &HashSet<String>,
) {
	if !contains_str(block) {
		return;
	}
	let mut reserved = reserved_set(extra_reserved);
	reserved.extend(table.globals.iter().cloned());
	reserved.extend(table.syms.iter().map(|s| s.name.clone()));

	let key: Vec<u8> = (0..KEY_LEN).map(|_| rng.int(0, 255) as u8).collect();
	let (loader, dec_sym, dec_name) = build_loader(table, rng, &mut reserved, &key);
	let cfg = Cfg {
		key,
		dec: dec_sym,
		dec_name,
	};
	let new_block = xform_block(block, &cfg);
	*block = new_block;
	block.stmts.splice(0..0, loader);
}
