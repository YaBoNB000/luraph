//! L7 anti-tamper (container layer; our differentiator vs Luraph v15):
//!
//! Post-pass over the L5 container. The final `loadstring(DEC(...))()`
//! statement is wrapped with:
//!
//!   1. canary self-check — a tiny pure function that must return a
//!      build-specific value; patched environment -> silent trap
//!   2. container checksum — byte sum of the ciphertext (concat of the
//!      chunk literals) mod a prime vs the build-time expected value;
//!      tampered ciphertext -> silent trap BEFORE decryption
//!   3. error rewrite — uncaught inner errors are pcall'd; the line
//!      number in the message is remapped (+ random offset) and the
//!      error re-raised at level 0 (stack depth hides the wrapper)
//!   4. timing trap — os.clock around the whole protection+execute
//!      sequence; single-step / slow hook beyond the threshold ->
//!      silent trap
//!
//! All traps are silent `while true do end` (no message, no exit code
//! hint). All constants are seed-derived; all locals are fresh mangled
//! names (reserved set includes every existing symbol + the globals the
//! wrapper itself references, so nothing can shadow).

use crate::ast::*;
use crate::mangle::gen_name;
use crate::rng::Rng;
use crate::symtab::{Sym, SymTable};
use std::collections::{HashMap, HashSet};

/// Primes for the container checksum modulus.
const PRIMES: &[i64] = &[1009, 2003, 4001, 8191, 104729, 15485863, 20132659];

fn num(v: i64) -> Expr {
	Expr::Num {
		value: v as f64,
		isfloat: false,
	}
}

fn str_s(bytes: &[u8]) -> Expr {
	Expr::Str {
		bytes: bytes.to_vec(),
	}
}

fn global(name: &str) -> Expr {
	Expr::Ident {
		name: name.to_string(),
		sym: None,
	}
}

/// `table.fn(args)` or bare `table(args)` when fn_name is empty.
fn global_call(table_name: &str, fn_name: &str, args: Vec<Expr>) -> Expr {
	let func = if fn_name.is_empty() {
		global(table_name)
	} else {
		Expr::Dot {
			obj: Box::new(global(table_name)),
			name: fn_name.to_string(),
		}
	};
	Expr::Call {
		func: Box::new(func),
		args,
	}
}

/// Create a fresh symbol for the container layer.
fn fresh(
	table: &mut SymTable,
	rng: &mut Rng,
	reserved: &mut HashSet<String>,
) -> (SymId, String) {
	let name = gen_name(rng, reserved);
	reserved.insert(name.clone());
	let id = table.syms.len() as SymId;
	table.syms.push(Sym {
		name: name.clone(),
		is_param: false,
		keep_name: false,
	});
	(id, name)
}

fn ident(s: SymId, table: &SymTable) -> Expr {
	Expr::Ident {
		name: table.name_of(s).to_string(),
		sym: Some(s),
	}
}

fn local1(name: String, sym: SymId, value: Expr) -> Stmt {
	Stmt::Local {
		names: vec![name],
		syms: vec![sym],
		values: vec![Some(value)],
	}
}

fn trap() -> Stmt {
	Stmt::While {
		cond: Box::new(Expr::Bool { value: true }),
		body: Block {
			stmts: Vec::new(),
		},
	}
}

/// Match the L5 container's final statement: ( loadstring( DEC(C1..CN) ) )()
/// Returns (chunks, dec_call) of the final container call.
fn find_container(stmts: &[Stmt]) -> Option<(Vec<Expr>, Expr)> {
	match stmts.last() {
		Some(Stmt::ExprStmt(Expr::Call {
			func,
			args: inner_args,
			..
		})) if inner_args.is_empty() => match func.as_ref() {
			Expr::Call { func: ls, args: dec_args } if dec_args.len() == 1 => {
				let dec = &dec_args[0];
				if let Expr::Ident { name, sym } = ls.as_ref() {
					if name == "loadstring" && sym.is_none() {
					if let Expr::Call { args: chunks, .. } = dec {
						return Some((chunks.clone(), dec.clone()));
					}
					}
				}
				None
			}
			_ => None,
		},
		_ => None,
	}
}

/// Canary self-check: a tiny pure function that must return a
/// build-specific value.
fn canary(
	table: &mut SymTable,
	rng: &mut Rng,
	reserved: &mut HashSet<String>,
) -> Vec<Stmt> {
	let (t_fn, t_fn_name) = fresh(table, rng, reserved);
	let (t_a, _) = fresh(table, rng, reserved);
	let (t_b, _) = fresh(table, rng, reserved);
	let a = rng.int(100, 999);
	let m = rng.int(7, 99);
	let c = rng.int(0, 999);
	let p = rng.int(97, 997);
	let expected = (a * m + c).rem_euclid(p);

	let body = Block {
		stmts: vec![
			local1(
				table.name_of(t_a).to_string(),
				t_a,
				num(a),
			),
			local1(
				table.name_of(t_b).to_string(),
				t_b,
				Expr::Bin {
					op: BinOp::Add,
					l: Box::new(Expr::Bin {
						op: BinOp::Mul,
						l: Box::new(ident(t_a, table)),
						r: Box::new(num(m)),
					}),
					r: Box::new(num(c)),
				},
			),
			Stmt::Return(vec![Expr::Bin {
				op: BinOp::Mod,
				l: Box::new(ident(t_b, table)),
				r: Box::new(num(p)),
			}]),
		],
	};
	vec![
		Stmt::Local {
			names: vec![t_fn_name],
			syms: vec![t_fn],
			values: vec![Some(Expr::Function {
				params: Vec::new(),
				param_syms: Vec::new(),
				vararg: false,
				body,
			})],
		},
		Stmt::If {
			cond: Box::new(Expr::Bin {
				op: BinOp::Ne,
				l: Box::new(Expr::Call {
					func: Box::new(ident(t_fn, table)),
					args: Vec::new(),
				}),
				r: Box::new(num(expected)),
			}),
			thenb: Block { stmts: vec![trap()] },
			elsifs: Vec::new(),
			elseb: None,
		},
	]
}

/// The L7 execution wrapper: checksum -> pcall+rewrite -> timing trap.
/// (The canary is emitted separately at the top of the chunk.)
fn wrapper(
	cipher: &[u8],
	chunks_concat: Expr,
	dec_call: Expr,
	table: &mut SymTable,
	rng: &mut Rng,
	reserved: &mut HashSet<String>,
) -> Vec<Stmt> {
	// t0 starts everything (timing trap window)
	let (t_t0, t_t0_name) = fresh(table, rng, reserved);

	// --- container checksum (before decryption) ---------------------
	let (t_sum, t_sum_name) = fresh(table, rng, reserved);
	let (t_buf, t_buf_name) = fresh(table, rng, reserved);
	let (t_i, t_i_name) = fresh(table, rng, reserved);
	let prime = PRIMES[(rng.int(0, (PRIMES.len() - 1) as i64)) as usize];
	let mut expected: i64 = 0;
	for b in cipher {
		expected = expected.wrapping_add(*b as i64);
	}
	let expected = expected.rem_euclid(prime);

	let mut out: Vec<Stmt> = Vec::new();
	out.push(local1(t_t0_name, t_t0, global_call("os", "clock", Vec::new())));
	out.push(local1(t_buf_name, t_buf, chunks_concat));
	out.push(local1(t_sum_name, t_sum, num(0)));
	out.push(Stmt::ForNum {
		var: t_i_name,
		var_sym: t_i,
		start: Box::new(num(1)),
		limit: Box::new(Expr::Un {
			op: UnOp::Len,
			e: Box::new(ident(t_buf, table)),
		}),
		step: None,
		body: Block {
			stmts: vec![Stmt::Assign {
				targets: vec![ident(t_sum, table)],
				values: vec![Expr::Bin {
					op: BinOp::Add,
					l: Box::new(ident(t_sum, table)),
					r: Box::new(global_call(
						"string",
						"byte",
						vec![ident(t_buf, table), ident(t_i, table)],
					)),
				}],
			}],
		},
	});
	out.push(Stmt::If {
		cond: Box::new(Expr::Bin {
			op: BinOp::Ne,
			l: Box::new(Expr::Bin {
				op: BinOp::Mod,
				l: Box::new(ident(t_sum, table)),
				r: Box::new(num(prime)),
			}),
			r: Box::new(num(expected)),
		}),
		thenb: Block { stmts: vec![trap()] },
		elsifs: Vec::new(),
		elseb: None,
	});

	// --- execute under pcall + error rewrite ------------------------
	let (t_ok, t_ok_name) = fresh(table, rng, reserved);
	let (t_err, t_err_name) = fresh(table, rng, reserved);
	let (t_line, t_line_name) = fresh(table, rng, reserved);
	let offset = rng.int(1000, 99999);

	let run = Expr::Call {
		// ( loadstring( DEC(C1..CN) ) )()
		func: Box::new(Expr::Call {
			func: Box::new(global("loadstring")),
			args: vec![dec_call],
		}),
		args: Vec::new(),
	};
	out.push(Stmt::Local {
		names: vec![t_ok_name, t_err_name],
		syms: vec![t_ok, t_err],
		values: vec![Some(Expr::Call {
			func: Box::new(global("pcall")),
			args: vec![Expr::Function {
				params: Vec::new(),
				param_syms: Vec::new(),
				vararg: false,
				body: Block {
					stmts: vec![Stmt::Return(vec![run])],
				},
			}],
		})],
	});
	let err = || ident(t_err, table);
	let line = || ident(t_line, table);
	out.push(Stmt::If {
		cond: Box::new(Expr::Un {
			op: UnOp::Not,
			e: Box::new(ident(t_ok, table)),
		}),
		thenb: Block {
			stmts: vec![
				// only string errors carry a location
				Stmt::If {
					cond: Box::new(Expr::Bin {
						op: BinOp::Eq,
						l: Box::new(global_call("type", "", vec![err()])),
						r: Box::new(str_s(b"string")),
					}),
					thenb: Block {
						stmts: vec![
							local1(
								t_line_name.clone(),
								t_line,
								Expr::Method {
									obj: Box::new(err()),
									name: "match".to_string(),
									args: vec![str_s(b"[(:](%d+)")],
								},
							),
							Stmt::If {
								cond: Box::new(line()),
								thenb: Block {
									stmts: vec![Stmt::Assign {
										targets: vec![err()],
										values: vec![Expr::Method {
											obj: Box::new(err()),
											name: "gsub".to_string(),
											args: vec![
												str_s(b"([(:])%d+"),
												Expr::Bin {
													op: BinOp::Concat,
													l: Box::new(str_s(b"%1")),
													r: Box::new(Expr::Bin {
														op: BinOp::Add,
														l: Box::new(global_call(
															"tonumber",
															"",
															vec![line()],
														)),
														r: Box::new(num(offset)),
													}),
												},
												num(1),
											],
										}],
									}],
								},
								elsifs: Vec::new(),
								elseb: None,
							},
						],
					},
					elsifs: Vec::new(),
					elseb: None,
				},
				Stmt::ExprStmt(Expr::Call {
					func: Box::new(global("error")),
					args: vec![err(), num(0)],
				}),
			],
		},
		elsifs: Vec::new(),
		elseb: None,
	});

	// --- timing trap (single-step / slow hook) -----------------------
	let threshold = rng.int(5, 15);
	out.push(Stmt::If {
		cond: Box::new(Expr::Bin {
			op: BinOp::Gt,
			l: Box::new(Expr::Bin {
				op: BinOp::Sub,
				l: Box::new(global_call("os", "clock", Vec::new())),
				r: Box::new(ident(t_t0, table)),
			}),
			r: Box::new(num(threshold)),
		}),
		thenb: Block { stmts: vec![trap()] },
		elsifs: Vec::new(),
		elseb: None,
	});

	out
}

pub fn apply_antidbg(
	block: &mut Block,
	table: &mut SymTable,
	rng: &mut Rng,
	reserved: &mut HashSet<String>,
) {
	let mut rsv = reserved.clone();
	rsv.extend(table.globals.iter().cloned());
	rsv.extend(table.syms.iter().map(|s| s.name.clone()));
	// the wrapper references these globals — the container must never
	// declare locals with these names
	for g in [
		"loadstring", "pcall", "error", "type", "tonumber", "os", "string",
	] {
		rsv.insert(g.to_string());
	}

	// canary always (protects even when L5 is disabled)
	let canary_stmts = canary(table, rng, &mut rsv);

	let mut new_stmts = Vec::new();
	new_stmts.extend(canary_stmts);

	// resolve the ciphertext bytes: the DEC call arguments are Idents of
	// the chunk locals; map them back to their Str literals
	let mut byte_map: HashMap<SymId, Vec<u8>> = HashMap::new();
	for s in &block.stmts {
		if let Stmt::Local { syms, values, .. } = s {
			for (sy, v) in syms.iter().zip(values.iter()) {
				if let Some(Expr::Str { bytes }) = v {
					byte_map.insert(*sy, bytes.clone());
				}
			}
		}
	}
	match find_container(&block.stmts) {
		Some((chunks, dec_call)) => {
			let mut cipher = Vec::new();
			for c in &chunks {
				if let Expr::Ident { sym: Some(s), .. } = c {
					if let Some(b) = byte_map.get(s) {
						cipher.extend(b.iter());
					}
				}
			}
			let concat = chunks
				.iter()
				.fold(None, |acc: Option<Expr>, c: &Expr| match acc {
					None => Some(c.clone()),
					Some(a) => Some(Expr::Bin {
						op: BinOp::Concat,
						l: Box::new(a),
						r: Box::new(c.clone()),
					}),
				})
				.expect("at least one ciphertext chunk");
			let wrap = wrapper(&cipher, concat, dec_call, table, rng, &mut rsv);
			for s in block.stmts.iter().take(block.stmts.len() - 1) {
				new_stmts.push(s.clone());
			}
			new_stmts.extend(wrap);
		}
		None => {
			// no L5 container (e.g. --no-body): canary only
			new_stmts.extend(block.stmts.iter().cloned());
		}
	}
	block.stmts = new_stmts;
}
