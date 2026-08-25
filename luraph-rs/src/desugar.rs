//! L3 prerequisite: loop desugaring. Rewrites for-numeric / for-generic /
//! repeat-until into plain `while true` loops (5.1-safe, no goto needed) so
//! the CFG flattener sees a uniform world: simple stmts / if / while /
//! return / break / continue.
//!
//! Semantics preserved exactly (verified against Lua 5.1 reference):
//! - numeric for: body runs first with i=start, THEN i+=step, THEN check
//! - generic for: iterator protocol (iter, state, ctrl); the iterator's LAST
//!   return value is the new control; stop when first value == nil
//! - repeat: body, then check

use crate::ast::*;
use crate::symtab::{Sym, SymTable};

pub fn desugar(block: &mut Block, table: &mut SymTable) {
	let mut tmps: Vec<String> = Vec::new();
	desugar_block(block, table, &mut tmps);
}

fn new_sym(table: &mut SymTable, name: &str) -> SymId {
	let id = table.syms.len() as SymId;
	table.syms.push(Sym {
		name: name.to_string(),
		is_param: false,
		keep_name: false,
	});
	id
}

fn desugar_block(block: &mut Block, table: &mut SymTable, tmps: &mut Vec<String>) {
	let mut new_stmts = Vec::with_capacity(block.stmts.len());
	for stmt in block.stmts.drain(..) {
		let desugared = desugar_stmt(stmt, table, tmps);
		new_stmts.extend(desugared);
	}
	block.stmts = new_stmts;
}

fn desugar_stmt(stmt: Stmt, table: &mut SymTable, tmps: &mut Vec<String>) -> Vec<Stmt> {
	match stmt {
		Stmt::ForNum {
			var,
			var_sym,
			start,
			limit,
			step,
			mut body,
		} => {
			// nested loops inside the body must be desugared first
			desugar_block(&mut body, table, tmps);
			let step = step.map(|b| *b);
			// temporaries
			let (n_lim, s_lim) = mk_local(table, "lim");
			let (n_stp, s_stp) = mk_local(table, "stp");
			let (n_cur, s_cur) = mk_local(table, "cur");
			let step_expr = step.unwrap_or_else(|| {
				Expr::Num {
					value: 1.0,
					isfloat: false,
				}
			});
			// local lim, stp, cur = limit, step, start
			let local = Stmt::Local {
				names: vec![n_lim.clone(), n_stp.clone(), n_cur.clone()],
				syms: vec![s_lim, s_stp, s_cur],
				values: vec![
					Some((*limit).clone()),
					Some(step_expr.clone()),
					Some((*start).clone()),
				],
			};
			let cur_i = ident(&n_cur, Some(s_cur));
			let lim_i = ident(&n_lim, Some(s_lim));
			let stp_i = ident(&n_stp, Some(s_stp));
			// FRESH loop variable per iteration (real Lua gives each
			// iteration a fresh loop variable; closures capture it
			// per-iteration)
			let fresh_var = Stmt::Local {
				names: vec![var.clone()],
				syms: vec![var_sym],
				values: vec![Some(cur_i.clone())],
			};
			// cur = cur + stp
			let inc = Stmt::Assign {
				targets: vec![cur_i.clone()],
				values: vec![Expr::Bin {
					op: BinOp::Add,
					l: Box::new(cur_i.clone()),
					r: Box::new(stp_i.clone()),
				}],
			};
			// if (stp >= 0 and var > lim) or (stp < 0 and var < lim) then break end
			let ge = Expr::Bin {
				op: BinOp::Ge,
				l: Box::new(stp_i.clone()),
				r: Box::new(Expr::Num { value: 0.0, isfloat: false }),
			};
			let gt = Expr::Bin {
					op: BinOp::Gt,
					l: Box::new(cur_i.clone()),
					r: Box::new(lim_i.clone()),
				};
			let lt0 = Expr::Bin {
				op: BinOp::Lt,
				l: Box::new(stp_i.clone()),
				r: Box::new(Expr::Num { value: 0.0, isfloat: false }),
			};
			let lt = Expr::Bin {
					op: BinOp::Lt,
					l: Box::new(cur_i),
					r: Box::new(lim_i),
				};
			let cond = Expr::Bin {
				op: BinOp::Or,
				l: Box::new(Expr::Bin {
					op: BinOp::And,
					l: Box::new(ge),
					r: Box::new(gt),
				}),
				r: Box::new(Expr::Bin {
					op: BinOp::And,
					l: Box::new(lt0),
					r: Box::new(lt),
				}),
			};
			let check = Stmt::If {
				cond: Box::new(cond),
				thenb: Block {
					stmts: vec![Stmt::Break],
				},
				elsifs: vec![],
				elseb: None,
			};
			let while_body = Block {
				stmts: {
					let mut v = vec![fresh_var];
					v.extend(body.stmts);
					v.push(inc);
					v.push(check);
					v
				},
			};
			let w = Stmt::While {
				cond: Box::new(Expr::Bool { value: true }),
				body: while_body,
			};
			vec![local, w]
		}
		Stmt::ForGen { vars, syms, iters, mut body } => {
			// nested loops inside the body must be desugared first
			desugar_block(&mut body, table, tmps);
			let (n_iter, s_iter) = mk_local(table, "it");
			let (n_state, s_state) = mk_local(table, "st");
			let (n_ctrl, s_ctrl) = mk_local(table, "ctl");
			// do
			//   local iter, state, ctrl = iters...
			//   while true do
			//     local v1..vN, c2 = iter(state, ctrl)
			//     if v1 == nil then break end
			//     ctrl = c2
			//     body
			//   end
			// end
		// `local iter, state, control = <the for-in expression list>` —
		// a single non-call expression is the default-iterator form
		// (`for ... in t` ≡ `for ... in next, t, nil`)
		let values: Vec<Option<Expr>> = if iters.len() == 1 && !is_call_expr(&iters[0]) {
			vec![
				Some(ident("next", None)),
				Some(iters[0].clone()),
				Some(Expr::Nil),
			]
		} else {
			iters.iter().map(|e| Some(e.clone())).collect()
		};
		let local_iter = Stmt::Local {
			names: vec![n_iter.clone(), n_state.clone(), n_ctrl.clone()],
			syms: vec![s_iter, s_state, s_ctrl],
			values,
		};
			let call = Expr::Call {
				func: Box::new(ident(&n_iter, Some(s_iter))),
				args: vec![
					ident(&n_state, Some(s_state)),
					ident(&n_ctrl, Some(s_ctrl)),
				],
			};
			// Lua for convention: the iterator's return values go directly to
			// the loop variables (1st..Nth); the 1st return value is the new
			// control value and nil stops the loop.
			// `local v1, ..., vN = iter(state, ctrl); if v1 == nil then break end
			//  ctrl = v1; body`
			let mut var_names = Vec::new();
			let mut var_syms = Vec::new();
			for (n, s) in vars.iter().zip(syms.iter()) {
				var_names.push(n.clone());
				var_syms.push(*s);
			}
			let local_vars = Stmt::Local {
				names: var_names,
				syms: var_syms,
				values: vec![Some(call)],
			};
			let first_var = ident(&vars[0], Some(syms[0]));
			let check = Stmt::If {
				cond: Box::new(Expr::Bin {
					op: BinOp::Eq,
					l: Box::new(first_var.clone()),
					r: Box::new(Expr::Nil),
				}),
				thenb: Block {
					stmts: vec![Stmt::Break],
				},
				elsifs: vec![],
				elseb: None,
			};
			let ctrl_assign = Stmt::Assign {
				targets: vec![ident(&n_ctrl, Some(s_ctrl))],
				values: vec![first_var],
			};
			let while_body = Block {
				stmts: {
					let mut v = Vec::new();
					v.push(local_vars);
					v.push(check);
					v.push(ctrl_assign);
					v.extend(body.stmts);
					v
				},
			};
			let w = Stmt::While {
				cond: Box::new(Expr::Bool { value: true }),
				body: while_body,
			};
			vec![Stmt::Do(Block {
				stmts: vec![local_iter, w],
			})]
		}
		Stmt::Repeat { mut body, cond } => {
			// nested loops inside the body must be desugared first
			desugar_block(&mut body, table, tmps);
			// while true do body; if cond then break end end
			let check = Stmt::If {
				cond,
				thenb: Block {
					stmts: vec![Stmt::Break],
				},
				elsifs: vec![],
				elseb: None,
			};
			let while_body = Block {
				stmts: {
					let mut v = body.stmts;
					v.push(check);
					v
				},
			};
			vec![Stmt::While {
				cond: Box::new(Expr::Bool { value: true }),
				body: while_body,
			}]
		}
		other => {
			// recurse into nested blocks/functions (incl. Local values with
			// anonymous functions containing loops)
			let mut s = other;
			recurse_stmt(&mut s, table, tmps);
			vec![s]
		}
	}
}

fn mk_local(table: &mut SymTable, prefix: &str) -> (String, SymId) {
	let name = format!("_{}_{}", prefix, table.syms.len());
	let id = new_sym(table, &name);
	(name, id)
}

fn is_call_expr(e: &Expr) -> bool {
	matches!(e, Expr::Call { .. } | Expr::Method { .. })
}

fn ident(name: &str, sym: Option<SymId>) -> Expr {
	Expr::Ident {
		name: name.to_string(),
		sym,
	}
}


/// Recurse desugaring into nested blocks/functions of non-loop statements.
fn recurse_stmt(s: &mut Stmt, table: &mut SymTable, tmps: &mut Vec<String>) {
	match s {
		Stmt::If { thenb, elsifs, elseb, .. } => {
			desugar_block(thenb, table, tmps);
			for (_, b) in elsifs.iter_mut() {
				desugar_block(b, table, tmps);
			}
			if let Some(b) = elseb {
				desugar_block(b, table, tmps);
			}
		}
		Stmt::While { body, .. } => desugar_block(body, table, tmps),
		Stmt::Do(b) => desugar_block(b, table, tmps),
		Stmt::LocalFunc { func, .. } => desugar_block(&mut func.body, table, tmps),
		Stmt::FuncDecl { func, .. } => desugar_block(&mut func.body, table, tmps),
		Stmt::Local { values, .. } => {
			for v in values.iter_mut() {
				if let Some(e) = v {
					recurse_expr(e, table, tmps);
				}
			}
		}
		Stmt::Assign { values, .. } => {
			for v in values.iter_mut() {
				recurse_expr(v, table, tmps);
			}
		}
		Stmt::ExprStmt(e) => recurse_expr(e, table, tmps),
		Stmt::Return(es) => {
			for e in es.iter_mut() {
				recurse_expr(e, table, tmps);
			}
		}
		_ => {}
	}
}

fn recurse_expr(e: &mut Expr, table: &mut SymTable, tmps: &mut Vec<String>) {
	match e {
		Expr::Function { body, .. } => desugar_block(body, table, tmps),
		Expr::Call { func, args } => {
			recurse_expr(func, table, tmps);
			for a in args.iter_mut() {
				recurse_expr(a, table, tmps);
			}
		}
		Expr::Method { obj, args, .. } => {
			recurse_expr(obj, table, tmps);
			for a in args.iter_mut() {
				recurse_expr(a, table, tmps);
			}
		}
		Expr::Bin { l, r, .. } => {
			recurse_expr(l, table, tmps);
			recurse_expr(r, table, tmps);
		}
		Expr::Un { e, .. } => recurse_expr(e, table, tmps),
		Expr::Table { fields } => {
			for f in fields.iter_mut() {
				match f {
					TableField::Array(e) => recurse_expr(e, table, tmps),
					TableField::Key { key, value } => {
						recurse_expr(key, table, tmps);
						recurse_expr(value, table, tmps);
					}
				}
			}
		}
		Expr::Dot { obj, .. } => recurse_expr(obj, table, tmps),
		Expr::Index { obj, idx } => {
			recurse_expr(obj, table, tmps);
			recurse_expr(idx, table, tmps);
		}
		_ => {}
	}
}
