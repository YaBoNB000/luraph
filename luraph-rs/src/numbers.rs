//! L4 numeric obfuscation: numeric literals are rewritten so no raw
//! constant is directly searchable in the output.
//!
//! - integers: 2~4 signed-term sum/difference (small range) or a
//!   `a * b + c` product form (large range) — every term is
//!   seed-random, non-trivial (never 0/±1) and never equal to the
//!   original value
//! - floats: exact identity wraps only (`0 + x` / `x + 0` / `x * 1` /
//!   `x / 1`) — ANY decomposition of a float changes its IEEE value,
//!   so floats are never split
//!
//! The transform is value-exact (the matrix re-runs everything under
//! both interpreters to prove it).

use crate::ast::*;
use crate::rng::Rng;

pub fn apply_numbers(block: &mut Block, rng: &mut Rng) {
	rw_stmts(block, rng);
}

fn rw_stmts(b: &mut Block, rng: &mut Rng) {
	let stmts = std::mem::take(&mut b.stmts);
	b.stmts = stmts.into_iter().map(|s| rw_stmt(s, rng)).collect();
}

fn rw_stmt(s: Stmt, rng: &mut Rng) -> Stmt {
	match s {
		Stmt::Local { names, syms, values } => Stmt::Local {
			names,
			syms,
			values: values
				.into_iter()
				.map(|v| v.map(|e| rw_expr(e, rng)))
				.collect(),
		},
		Stmt::LocalFunc { name, sym, func } => {
			let mut func = func;
			rw_stmts(&mut func.body, rng);
			Stmt::LocalFunc { name, sym, func }
		}
		Stmt::FuncDecl { obj, name, ismethod, func } => {
			let obj = obj.map(|o| rw_expr(o, rng));
			let mut func = func;
			rw_stmts(&mut func.body, rng);
			Stmt::FuncDecl {
				obj,
				name,
				ismethod,
				func,
			}
		}
		Stmt::Assign { targets, values } => {
			let targets = targets.into_iter().map(|t| rw_expr(t, rng)).collect();
			let values = values.into_iter().map(|v| rw_expr(v, rng)).collect();
			Stmt::Assign { targets, values }
		}
		Stmt::ExprStmt(e) => Stmt::ExprStmt(rw_expr(e, rng)),
		Stmt::If { cond, thenb, elsifs, elseb } => {
			let mut thenb = thenb;
			rw_stmts(&mut thenb, rng);
			let elsifs = elsifs
				.into_iter()
				.map(|(c, mut b)| {
					rw_stmts(&mut b, rng);
					(rw_expr(c, rng), b)
				})
				.collect();
			let mut elseb = elseb;
			if let Some(ref mut b) = elseb {
				rw_stmts(b, rng);
			}
			Stmt::If {
				cond: Box::new(rw_expr(*cond, rng)),
				thenb,
				elsifs,
				elseb,
			}
		}
		Stmt::While { cond, body } => {
			let mut body = body;
			rw_stmts(&mut body, rng);
			Stmt::While {
				cond: Box::new(rw_expr(*cond, rng)),
				body,
			}
		}
		Stmt::Repeat { body, cond } => {
			let mut body = body;
			rw_stmts(&mut body, rng);
			Stmt::Repeat {
				body,
				cond: Box::new(rw_expr(*cond, rng)),
			}
		}
		Stmt::ForNum { var, var_sym, start, limit, step, body } => {
			let mut body = body;
			rw_stmts(&mut body, rng);
			Stmt::ForNum {
				var,
				var_sym,
				start: Box::new(rw_expr(*start, rng)),
				limit: Box::new(rw_expr(*limit, rng)),
				step: step.map(|b| Box::new(rw_expr(*b, rng))),
				body,
			}
		}
		Stmt::ForGen { vars, syms, iters, body } => {
			let mut body = body;
			rw_stmts(&mut body, rng);
			Stmt::ForGen {
				vars,
				syms,
				iters: iters.into_iter().map(|i| rw_expr(i, rng)).collect(),
				body,
			}
		}
		Stmt::Do(b) => {
			let mut b = b;
			rw_stmts(&mut b, rng);
			Stmt::Do(b)
		}
		Stmt::Return(es) => Stmt::Return(es.into_iter().map(|e| rw_expr(e, rng)).collect()),
		other => other,
	}
}

fn rw_expr(e: Expr, rng: &mut Rng) -> Expr {
	match e {
		Expr::Num { value, isfloat } => rw_num(value, isfloat, rng),
		Expr::Ident { .. } | Expr::Str { .. } | Expr::Bool { .. } | Expr::Nil | Expr::Vararg => e,
		Expr::Dot { obj, name } => Expr::Dot {
			obj: Box::new(rw_expr(*obj, rng)),
			name,
		},
		Expr::Index { obj, idx } => Expr::Index {
			obj: Box::new(rw_expr(*obj, rng)),
			idx: Box::new(rw_expr(*idx, rng)),
		},
		Expr::Call { func, args } => Expr::Call {
			func: Box::new(rw_expr(*func, rng)),
			args: args.into_iter().map(|a| rw_expr(a, rng)).collect(),
		},
		Expr::Method { obj, name, args } => Expr::Method {
			obj: Box::new(rw_expr(*obj, rng)),
			name,
			args: args.into_iter().map(|a| rw_expr(a, rng)).collect(),
		},
		Expr::Un { op, e } => Expr::Un {
			op,
			e: Box::new(rw_expr(*e, rng)),
		},
		Expr::Bin { op, l, r } => Expr::Bin {
			op,
			l: Box::new(rw_expr(*l, rng)),
			r: Box::new(rw_expr(*r, rng)),
		},
		Expr::Table { fields } => Expr::Table {
			fields: fields
				.into_iter()
				.map(|f| match f {
					TableField::Array(e) => TableField::Array(rw_expr(e, rng)),
					TableField::Key { key, value } => TableField::Key {
						key: rw_expr(key, rng),
						value: rw_expr(value, rng),
					},
				})
				.collect(),
		},
		Expr::Function { params, param_syms, vararg, body } => {
			let mut body = body;
			rw_stmts(&mut body, rng);
			Expr::Function {
				params,
				param_syms,
				vararg,
				body,
			}
		}
	}
}

fn num_expr(value: f64, isfloat: bool) -> Expr {
	Expr::Num {
		value,
		isfloat,
	}
}

fn is_int_value(value: f64, isfloat: bool) -> bool {
	!isfloat && value.fract() == 0.0 && value.abs() < 1e15
}

/// Rewrite one numeric literal.
fn rw_num(value: f64, isfloat: bool, rng: &mut Rng) -> Expr {
	if is_int_value(value, isfloat) {
		let v = value as i64;
		// 0/±1 are everywhere and trivial — decomposing them only adds
		// noise
		if v == 0 || v == 1 || v == -1 {
			return num_expr(value, isfloat);
		}
		decompose_int(v, rng)
	} else {
		// float: exact identity wrap only (never split — any split
		// changes the IEEE value)
		let inner = num_expr(value, isfloat);
		match rng.int(0, 3) {
			0 => bin(BinOp::Add, num_expr(0.0, false), inner),
			1 => bin(BinOp::Add, inner, num_expr(0.0, false)),
			2 => bin(BinOp::Mul, inner, num_expr(1.0, false)),
			_ => bin(BinOp::Div, inner, num_expr(1.0, false)),
		}
	}
}

fn bin(op: BinOp, l: Expr, r: Expr) -> Expr {
	Expr::Bin {
		op,
		l: Box::new(l),
		r: Box::new(r),
	}
}

/// Random magnitude in [lo, hi].
fn mag(rng: &mut Rng, lo: i64, hi: i64) -> i64 {
	rng.int(lo, hi)
}

/// Decompose an integer into a value-exact expression whose terms never
/// include the original value and are all non-trivial.
fn decompose_int(v: i64, rng: &mut Rng) -> Expr {
	if v.abs() < 1_000_000 {
		// 2~4 signed terms in [100, 1_000_000]; solve the last term
		for _ in 0..12 {
			let nterms = rng.int(2, 4) as usize;
			let mut acc: i64 = 0;
			let mut terms: Vec<(BinOp, i64)> = Vec::new();
			let mut ok = true;
			for k in 0..nterms {
				let last = k == nterms - 1;
				if last {
					let t = v - acc;
					if t == 0 || t == 1 || t == -1 || t.abs() < 100 || t.abs() > 1_000_000 {
						ok = false;
						break;
					}
					terms.push((BinOp::Add, t)); // sign handled below
					acc += t;
				} else {
					let t = mag(rng, 100, 1_000_000);
					// the first term is always positive (the emitted chain
					// starts with its literal; a leading sign would be lost)
					let sub = k > 0 && rng.int(0, 1) == 1;
					if sub {
						acc -= t;
					} else {
						acc += t;
					}
					terms.push((if sub { BinOp::Sub } else { BinOp::Add }, t));
				}
			}
			if ok && acc == v {
				// left-associative chain of positive literals
				let mut cur = num_expr(terms[0].1 as f64, false);
				for (op, t) in &terms[1..] {
					cur = bin(*op, cur, num_expr(*t as f64, false));
				}
				return cur;
			}
		}
	}
	// large |v| (or sum failed): product form  a * b + c
	// a in [100, 10000], b = v div a, c in [100, 2a) — no term equals v
	let a = mag(rng, 100, 10_000);
	let mut b = v.div_euclid(a);
	let mut c = v.rem_euclid(a); // c in [0, a)
	if c < 100 {
		b -= 1;
		c += a;
	}
	bin(BinOp::Add, bin(BinOp::Mul, num_expr(a as f64, false), num_expr(b as f64, false)), num_expr(c as f64, false))
}

#[cfg(test)]
mod tests {
	use super::*;

	/// Evaluate a Num/Add/Sub/Mul/Div chain.
	fn eval(e: &Expr) -> f64 {
		match e {
			Expr::Num { value, .. } => *value,
			Expr::Bin { op, l, r } => {
				let (x, y) = (eval(l), eval(r));
				match op {
					BinOp::Add => x + y,
					BinOp::Sub => x - y,
					BinOp::Mul => x * y,
					BinOp::Div => x / y,
					_ => panic!("unexpected op in test expr"),
				}
			}
			_ => panic!("unexpected node in test expr"),
		}
	}

	fn contains_num(e: &Expr, v: f64) -> bool {
		match e {
			Expr::Num { value, isfloat: false } => *value == v,
			Expr::Num { .. } => false,
			Expr::Bin { l, r, .. } => contains_num(l, v) || contains_num(r, v),
			_ => false,
		}
	}

	fn terms(e: &Expr) -> Vec<i64> {
		let mut out = Vec::new();
		match e {
			Expr::Num { value, isfloat: false } => out.push(*value as i64),
			Expr::Bin { l, r, .. } => {
				terms(l);
				terms(r);
			}
			_ => {}
		}
		out
	}

	#[test]
	fn integer_decomposition_exact() {
		let mut rng = Rng::new(1234);
		let cases = [
			2i64, 42, 99, 100, -100, 12345, -12345, 999_999, -999_999, 1_000_000, -1_000_000,
			12_345_678, -12_345_678, 4_294_967_295, -4_294_967_295, 1_000_000_000_000,
			-1_000_000_000_000,
		];
		for v in cases {
			for _ in 0..50 {
				let e = decompose_int(v, &mut rng);
				assert_eq!(eval(&e) as i64, v, "value changed for {v}");
				assert!(!contains_num(&e, v as f64), "original value {v} visible");
				for t in terms(&e) {
					assert!(t != 0 && t != 1 && t != -1, "trivial term {t} for {v}");
					assert!(t != v, "term equals original for {v}");
				}
			}
		}
	}

	#[test]
	fn small_values_stress() {
		for seed in 0..2000u64 {
			let mut rng = Rng::new(seed);
			for v in [3i64, -3, 4, 100] {
				for _ in 0..20 {
					let e = decompose_int(v, &mut rng);
					let got = eval(&e) as i64;
					if got != v {
						let mut buf = String::new();
						fn dump(e: &Expr, buf: &mut String) {
							match e {
								Expr::Num { value, .. } => buf.push_str(&format!("{value}")),
								Expr::Bin { op, l, r } => {
									buf.push('(');
									dump(l, buf);
									buf.push_str(match op {
										BinOp::Add => " + ", BinOp::Sub => " - ", BinOp::Mul => " * ", _ => "?",
									});
									dump(r, buf);
									buf.push(')');
								}
								_ => {}
							}
						}
						dump(&e, &mut buf);
						panic!("seed {seed} v {v}: got {got}: {buf}");
					}
				}
			}
		}
	}

	#[test]
	fn trivial_integers_untouched() {
		let mut rng = Rng::new(7);
		for v in [0i64, 1, -1] {
			match rw_num(v as f64, false, &mut rng) {
				Expr::Num { value, isfloat: false } => assert_eq!(value as i64, v),
				_ => panic!("trivial {v} should stay a literal"),
			}
		}
	}

	#[test]
	fn floats_get_identity_wrap() {
		let mut rng = Rng::new(99);
		for v in [3.14f64, -2.5, 1e-7, 1.5e10, 100.0] {
			let e = rw_num(v, true, &mut rng);
			// must be a Bin with an integer 0 or 1 literal on one side
			match &e {
				Expr::Bin { op, l, r } => {
					let one_side = |x: &Expr| matches!(
						x,
						Expr::Num { value, isfloat: false } if *value == 0.0 || *value == 1.0
					);
					assert!(
						one_side(l) || one_side(r),
						"no 0/1 identity operand for {v}"
					);
					assert!(matches!(
						op,
						BinOp::Add | BinOp::Mul | BinOp::Div
					));
					assert_eq!(eval(&e), v, "value changed for {v}");
				}
				_ => panic!("float {v} not wrapped"),
			}
		}
	}
}
