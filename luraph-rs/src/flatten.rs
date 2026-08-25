//! L3 control flow flattening (the Luraph-style signature transform).
//!
//! Every function body (and the top-level chunk) is compiled to a block
//! graph and emitted as a state machine:
//!
//!   local st = <random_id>
//!   local <hoisted locals>
//!   while true do
//!     if st == <id_A> then ... st = <id_B>
//!     elseif st == <id_C> then ...
//!     ... (branch order shuffled)
//!   end
//!
//! Scope-soundness (the machine's arms are sibling scopes in Lua, so a
//! `local` declared in one arm is invisible to all the others):
//!
//! - A local referenced from another arm — directly, from a closure
//!   defined in another arm, or from a loop body — is declared once at
//!   the machine top (its original block keeps a plain assignment).
//!   A local used only within its own arm stays arm-local.
//! - Mangled names are unique across the program and never collide with
//!   global names (see mangle.rs), so machine-top declarations can never
//!   shadow a global use.
//!
//! Loops are NESTED sub-machines: a loop is an opaque node in the
//! enclosing graph; its arm emits
//!
//!   <per-pass `local` declarations (fresh EVERY iteration)>
//!   local st_l = <first_body_id>
//!   local broke = false
//!   while true do
//!     if st_l == ... then ... st_l = ...   (body blocks, shuffled)
//!     ...
//!   end
//!   <loop bookkeeping: increment / re-cond / exit>
//!
//! which gives loop variables and body locals true per-iteration
//! freshness (closures created in iteration k capture iteration k's
//! values, exactly like real Lua `for` loops), while `break` and
//! `continue` are plain graph edges:
//!
//!   break    -> loop exit
//!   continue -> body end (for/while: re-pass = increment + re-cond,
//!               which is exactly Luau's continue semantics; repeat:
//!               the until-check, which lives inside the dispatch)
//!
//! for-numeric loops run in true Lua order: range check BEFORE the body
//! (an empty range runs 0 times), fresh loop variable per iteration,
//! increment after the body.

use crate::ast::*;
use crate::mangle::gen_name;
use crate::rng::Rng;
use crate::symtab::{Sym, SymTable};
use std::collections::{HashMap, HashSet, VecDeque};

// ---------------------------------------------------------------------------
// block graph
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
enum Succ {
	/// normal edge to block `x`
	B(usize),
	/// `break` — exits the innermost loop (inner graphs only)
	Brk,
	/// `continue` — placeholder, rewired to the loop's continue target
	/// (the body-end join, or the repeat's until-check) after the body is
	/// built
	Cont,
}

#[derive(Debug, Clone)]
enum LoopKind {
	While { cond: Expr },
	Repeat { cond: Expr },
	/// init: `local cur, lim, stp = start, limit, step` (one graph block,
	/// evaluation order = start, limit, step, like the original for)
	ForNum { var: SymId, lim: SymId, stp: SymId, cur: SymId },
	/// init: `local it, stt, ctl = <iters...>`
	ForGen { vars: Vec<SymId>, it: SymId, stt: SymId, ctl: SymId },
}

#[derive(Debug, Clone)]
struct LoopNode {
	kind: LoopKind,
	/// the loop body's block graph (a separate machine)
	blocks: Vec<Blk>,
	start: usize,
}

#[derive(Debug, Clone)]
enum BlkBody {
	Stmt(Stmt),
	Cond(Expr),
	Join,
	Return(Vec<Expr>),
	/// opaque loop (its body is a nested sub-machine)
	Loop(Box<LoopNode>),
}

#[derive(Debug, Clone)]
struct Blk {
	body: BlkBody,
	succ: Vec<Succ>,
}

struct Ctx {
	blocks: Vec<Blk>,
	/// true while building a loop body (break/continue are allowed)
	inner: bool,
}

impl Ctx {
	fn new(inner: bool) -> Ctx {
		Ctx {
			blocks: Vec::new(),
			inner,
		}
	}
	fn push(&mut self, body: BlkBody) -> usize {
		let i = self.blocks.len();
		self.blocks.push(Blk {
			body,
			succ: Vec::new(),
		});
		i
	}
}

/// What follows a statement chain that falls off the end.
enum Cont {
	/// create a continuation join block (top-level chunk/function)
	Join,
	/// create a body-end join block (loop body: while/for)
	Done,
	/// create the repeat's until-check cond block (loop body: repeat)
	Check { cond: Expr },
}

/// Expand `Do` statements inline (they carry no bindings).
fn expand(stmts: &[Stmt]) -> Vec<Stmt> {
	let mut out = Vec::with_capacity(stmts.len());
	for s in stmts {
		match s {
			Stmt::Do(b) => out.extend(b.stmts.iter().cloned()),
			other => out.push(other.clone()),
		}
	}
	out
}

fn num(v: u32) -> Expr {
	Expr::Num {
		value: v as f64,
		isfloat: false,
	}
}

fn ident(sym: SymId, table: &SymTable) -> Expr {
	Expr::Ident {
		name: table.name_of(sym).to_string(),
		sym: Some(sym),
	}
}

fn new_rand_sym(
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

// ---------------------------------------------------------------------------
// graph construction
// ---------------------------------------------------------------------------

/// Build the block graph for a statement list. Returns (start, cont) where
/// cont is the block created by `cont` (join / done-join / check-cond).
fn build_graph(
	ctx: &mut Ctx,
	stmts: &[Stmt],
	cont: Cont,
	table: &mut SymTable,
	rng: &mut Rng,
	reserved: &mut HashSet<String>,
) -> (usize, usize) {
	let flat = expand(stmts);
	let start = ctx.push(BlkBody::Join);
	let mut cur: Option<usize> = Some(start);
	for stmt in flat.iter() {
		cur = match cur {
			None => None, // unreachable tail
			Some(c) => emit_stmt(ctx, c, stmt, table, rng, reserved),
		};
	}
	let cont_blk = match cont {
		Cont::Join | Cont::Done => {
			let c = ctx.push(BlkBody::Join);
			if let Some(x) = cur {
				ctx.blocks[x].succ.push(Succ::B(c));
			}
			c
		}
		Cont::Check { cond } => {
			let check = ctx.push(BlkBody::Cond(cond));
			// repeat: cond true -> EXIT (break), cond false -> re-run body
			ctx.blocks[check].succ = vec![Succ::Brk, Succ::B(start)];
			if let Some(x) = cur {
				ctx.blocks[x].succ.push(Succ::B(check));
			}
			check
		}
	};
	(start, cont_blk)
}

/// Emit one statement into the chain. Returns the new current position,
/// or None when the chain dies (return/break/continue).
fn emit_stmt(
	ctx: &mut Ctx,
	cur: usize,
	stmt: &Stmt,
	table: &mut SymTable,
	rng: &mut Rng,
	reserved: &mut HashSet<String>,
) -> Option<usize> {
	match stmt {
		Stmt::Return(es) => {
			let b = ctx.push(BlkBody::Return(es.clone()));
			ctx.blocks[cur].succ.push(Succ::B(b));
			None
		}
		Stmt::Break => {
			assert!(ctx.inner, "break outside loop (parser must reject)");
			ctx.blocks[cur].succ.push(Succ::Brk);
			None
		}
		Stmt::Continue => {
			assert!(ctx.inner, "continue outside loop (parser must reject)");
			ctx.blocks[cur].succ.push(Succ::Cont);
			None
		}
		Stmt::If { cond, thenb, elsifs, elseb } => {
			// bodies first (stable indices), then cond blocks, then wire
			let (t_s, t_e) = build_graph(ctx, &thenb.stmts, Cont::Join, table, rng, reserved);
			let mut els = Vec::new();
			for (_, b) in elsifs.iter() {
				els.push(build_graph(ctx, &b.stmts, Cont::Join, table, rng, reserved));
			}
			let else_pair = match elseb {
				Some(b) => Some(build_graph(ctx, &b.stmts, Cont::Join, table, rng, reserved)),
				None => None,
			};
			let if_cond = ctx.push(BlkBody::Cond((**cond).clone()));
			let c1: Vec<usize> = elsifs
				.iter()
				.map(|(c, _)| ctx.push(BlkBody::Cond(c.clone())))
				.collect();
			let exit = ctx.push(BlkBody::Join);

			ctx.blocks[cur].succ.push(Succ::B(if_cond));
			let else_target_0 = match c1.first() {
				Some(c) => *c,
				None => match else_pair {
					Some((s, _)) => s,
					None => exit,
				},
			};
			ctx.blocks[if_cond].succ = vec![Succ::B(t_s), Succ::B(else_target_0)];
			ctx.blocks[t_e].succ = vec![Succ::B(exit)];
			for (i, c) in c1.iter().enumerate() {
				let target = match c1.get(i + 1) {
					Some(nc) => *nc,
					None => match else_pair {
						Some((s, _)) => s,
						None => exit,
					},
				};
				ctx.blocks[*c].succ = vec![Succ::B(els[i].0), Succ::B(target)];
				ctx.blocks[els[i].1].succ = vec![Succ::B(exit)];
			}
			if let Some((s, e)) = else_pair {
				ctx.blocks[e].succ = vec![Succ::B(exit)];
				let _ = s;
			}
			Some(exit)
		}
		Stmt::While { .. } | Stmt::Repeat { .. } | Stmt::ForNum { .. } | Stmt::ForGen { .. } => {
			make_loop(ctx, cur, stmt, table, rng, reserved)
		}
		other => {
			let b = ctx.push(BlkBody::Stmt(other.clone()));
			ctx.blocks[cur].succ.push(Succ::B(b));
			Some(b)
		}
	}
}

/// Turn a loop statement into: [init block] -> loop node -> exit join.
/// The loop body is built as a separate (nested) block graph.
fn make_loop(
	ctx: &mut Ctx,
	cur: usize,
	stmt: &Stmt,
	table: &mut SymTable,
	rng: &mut Rng,
	reserved: &mut HashSet<String>,
) -> Option<usize> {
	let (kind, init_local) = match stmt {
		Stmt::While { cond, .. } => (LoopKind::While { cond: (**cond).clone() }, None),
		Stmt::Repeat { cond, .. } => (LoopKind::Repeat { cond: (**cond).clone() }, None),
		Stmt::ForNum {
			var_sym,
			start,
			limit,
			step,
			..
		} => {
			let (s_cur, _) = new_rand_sym(table, rng, reserved);
			let (s_lim, _) = new_rand_sym(table, rng, reserved);
			let (s_stp, _) = new_rand_sym(table, rng, reserved);
			let step = step
				.as_ref()
				.map(|b| (**b).clone())
				.unwrap_or(Expr::Num {
					value: 1.0,
					isfloat: false,
				});
			// `local cur, lim, stp = start, limit, step` — value evaluation
			// order matches the original for (start, limit, step)
			let init = Stmt::Local {
				names: vec![
					table.name_of(s_cur).to_string(),
					table.name_of(s_lim).to_string(),
					table.name_of(s_stp).to_string(),
				],
				syms: vec![s_cur, s_lim, s_stp],
				values: vec![Some((**start).clone()), Some((**limit).clone()), Some(step)],
			};
			(
				LoopKind::ForNum {
					var: *var_sym,
					lim: s_lim,
					stp: s_stp,
					cur: s_cur,
				},
				Some(init),
			)
		}
		Stmt::ForGen { syms, iters, .. } => {
			let (s_it, _) = new_rand_sym(table, rng, reserved);
			let (s_stt, _) = new_rand_sym(table, rng, reserved);
			let (s_ctl, _) = new_rand_sym(table, rng, reserved);
			// it, stt, ctl = <iterator expression list> — a single
			// non-call expression is the iterator value itself (in
			// 5.1 a bare table errors at the call, like the host; the
			// Luau `for ... in t` form is normalized to `next, t` at
			// parse time)
			let init = Stmt::Local {
				names: vec![
					table.name_of(s_it).to_string(),
					table.name_of(s_stt).to_string(),
					table.name_of(s_ctl).to_string(),
				],
				syms: vec![s_it, s_stt, s_ctl],
				values: iters.iter().map(|e| Some(e.clone())).collect(),
			};
			(
				LoopKind::ForGen {
					vars: syms.clone(),
					it: s_it,
					stt: s_stt,
					ctl: s_ctl,
				},
				Some(init),
			)
		}
		_ => unreachable!("make_loop called on a non-loop"),
	};

	// init block (for-loops only), in the enclosing graph
	let mut prev = cur;
	if let Some(init) = init_local {
		let b = ctx.push(BlkBody::Stmt(init));
		ctx.blocks[cur].succ.push(Succ::B(b));
		prev = b;
	}

	// loop node + exit join in the enclosing graph
	let node_blk = ctx.push(BlkBody::Loop(Box::new(LoopNode {
		kind: kind.clone(),
		blocks: Vec::new(),
		start: 0,
	})));
	ctx.blocks[prev].succ.push(Succ::B(node_blk));
	let exit_blk = ctx.push(BlkBody::Join);
	ctx.blocks[node_blk].succ.push(Succ::B(exit_blk));

	// the nested body graph
	let body = match stmt {
		Stmt::While { body, .. }
		| Stmt::Repeat { body, .. }
		| Stmt::ForNum { body, .. }
		| Stmt::ForGen { body, .. } => body,
		_ => unreachable!(),
	};
	let cont = match &kind {
		LoopKind::While { .. } | LoopKind::ForNum { .. } | LoopKind::ForGen { .. } => Cont::Done,
		LoopKind::Repeat { cond } => Cont::Check { cond: cond.clone() },
	};
	let mut ictx = Ctx::new(true);
	let (start, cont_blk) = build_graph(&mut ictx, &body.stmts, cont, table, rng, reserved);

	// rewire continue placeholders -> the loop's continue target
	// (body-end join for while/for, until-check cond for repeat)
	for b in ictx.blocks.iter_mut() {
		b.succ = b
			.succ
			.iter()
			.map(|s| match s {
				Succ::Cont => Succ::B(cont_blk),
				other => other.clone(),
			})
			.collect();
	}

	let node = LoopNode {
		kind,
		blocks: ictx.blocks,
		start,
	};
	if let BlkBody::Loop(ref mut ln) = ctx.blocks[node_blk].body {
		*ln = Box::new(node);
	}
	Some(exit_blk)
}

fn reachable(blocks: &[Blk], start: usize) -> HashSet<usize> {
	let mut seen: HashSet<usize> = HashSet::new();
	let mut q: VecDeque<usize> = VecDeque::new();
	q.push_back(start);
	seen.insert(start);
	while let Some(i) = q.pop_front() {
		for &s in &blocks[i].succ {
			if let Succ::B(x) = s {
				if seen.insert(x) {
					q.push_back(x);
				}
			}
		}
	}
	seen
}

// ---------------------------------------------------------------------------
// symbol reference collection
// ---------------------------------------------------------------------------

/// Symbols referenced directly by an expression (does NOT descend into
/// nested function bodies — those are closures, collected separately).
fn collect_expr_syms(e: &Expr, out: &mut HashSet<SymId>) {
	match e {
		Expr::Ident { sym: Some(s), .. } => {
			out.insert(*s);
		}
		Expr::Dot { obj, .. } => collect_expr_syms(obj, out),
		Expr::Index { obj, idx } => {
			collect_expr_syms(obj, out);
			collect_expr_syms(idx, out);
		}
		Expr::Call { func, args } => {
			collect_expr_syms(func, out);
			for a in args {
				collect_expr_syms(a, out);
			}
		}
		Expr::Method { obj, args, .. } => {
			collect_expr_syms(obj, out);
			for a in args {
				collect_expr_syms(a, out);
			}
		}
		Expr::Bin { l, r, .. } => {
			collect_expr_syms(l, out);
			collect_expr_syms(r, out);
		}
		Expr::Un { e, .. } => collect_expr_syms(e, out),
		Expr::Table { fields } => {
			for f in fields {
				match f {
					TableField::Array(e) => collect_expr_syms(e, out),
					TableField::Key { key, value } => {
						collect_expr_syms(key, out);
						collect_expr_syms(value, out);
					}
				}
			}
		}
		_ => {}
	}
}

/// Like collect_expr_syms but descends into nested function bodies (used
/// for loop-body reference sets, where a closure inside the body still
/// needs the enclosing machine to hoist its locals).
fn collect_expr_syms_deep(e: &Expr, out: &mut HashSet<SymId>) {
	match e {
		Expr::Function { body, .. } => {
			collect_block_syms_deep(body, out);
		}
		other => {
			let mut tmp = HashSet::new();
			collect_expr_syms(other, &mut tmp);
			out.extend(tmp);
			match other {
				Expr::Dot { obj, .. } => collect_expr_syms_deep(obj, out),
				Expr::Index { obj, idx } => {
					collect_expr_syms_deep(obj, out);
					collect_expr_syms_deep(idx, out);
				}
				Expr::Call { func, args } => {
					collect_expr_syms_deep(func, out);
					for a in args {
						collect_expr_syms_deep(a, out);
					}
				}
				Expr::Method { obj, args, .. } => {
					collect_expr_syms_deep(obj, out);
					for a in args {
						collect_expr_syms_deep(a, out);
					}
				}
				Expr::Bin { l, r, .. } => {
					collect_expr_syms_deep(l, out);
					collect_expr_syms_deep(r, out);
				}
				Expr::Un { e, .. } => collect_expr_syms_deep(e, out),
				Expr::Table { fields } => {
					for f in fields {
						match f {
							TableField::Array(e) => collect_expr_syms_deep(e, out),
							TableField::Key { key, value } => {
								collect_expr_syms_deep(key, out);
								collect_expr_syms_deep(value, out);
							}
						}
					}
				}
				_ => {}
			}
		}
	}
}

fn collect_stmt_syms_deep(s: &Stmt, out: &mut HashSet<SymId>) {
	match s {
		Stmt::Local { values, .. } => {
			for v in values {
				if let Some(e) = v {
					collect_expr_syms_deep(e, out);
				}
			}
		}
		Stmt::LocalFunc { func, .. } => collect_block_syms_deep(&func.body, out),
		Stmt::FuncDecl { func, obj, .. } => {
			if let Some(o) = obj {
				collect_expr_syms_deep(o, out);
			}
			collect_block_syms_deep(&func.body, out);
		}
		Stmt::Assign { targets, values } => {
			for t in targets {
				collect_expr_syms_deep(t, out);
			}
			for v in values {
				collect_expr_syms_deep(v, out);
			}
		}
		Stmt::ExprStmt(e) => collect_expr_syms_deep(e, out),
		Stmt::If { cond, thenb, elsifs, elseb } => {
			collect_expr_syms_deep(cond, out);
			collect_block_syms_deep(thenb, out);
			for (c, b) in elsifs {
				collect_expr_syms_deep(c, out);
				collect_block_syms_deep(b, out);
			}
			if let Some(b) = elseb {
				collect_block_syms_deep(b, out);
			}
		}
		Stmt::While { cond, body } => {
			collect_expr_syms_deep(cond, out);
			collect_block_syms_deep(body, out);
		}
		Stmt::Repeat { body, cond } => {
			collect_block_syms_deep(body, out);
			collect_expr_syms_deep(cond, out);
		}
		Stmt::ForNum {
			start, limit, step, body, ..
		} => {
			collect_expr_syms_deep(start, out);
			collect_expr_syms_deep(limit, out);
			if let Some(st) = step {
				collect_expr_syms_deep(st, out);
			}
			collect_block_syms_deep(body, out);
		}
		Stmt::ForGen { iters, body, .. } => {
			for i in iters {
				collect_expr_syms_deep(i, out);
			}
			collect_block_syms_deep(body, out);
		}
		Stmt::Do(b) => collect_block_syms_deep(b, out),
		Stmt::Return(es) => {
			for e in es {
				collect_expr_syms_deep(e, out);
			}
		}
		_ => {}
	}
}

fn collect_block_syms_deep(b: &Block, out: &mut HashSet<SymId>) {
	for s in &b.stmts {
		collect_stmt_syms_deep(s, out);
	}
}

/// All symbols a loop node's emitted code references: the loop-kind
/// symbols (init temporaries, loop variable) plus everything referenced
/// anywhere in the nested body graph (closures included).
fn loop_node_refs(node: &LoopNode, out: &mut HashSet<SymId>) {
	match &node.kind {
		LoopKind::While { cond } | LoopKind::Repeat { cond } => collect_expr_syms_deep(cond, out),
		LoopKind::ForNum { var, lim, stp, cur } => {
			out.insert(*var);
			out.insert(*lim);
			out.insert(*stp);
			out.insert(*cur);
		}
		LoopKind::ForGen { vars, it, stt, ctl } => {
			for v in vars {
				out.insert(*v);
			}
			out.insert(*it);
			out.insert(*stt);
			out.insert(*ctl);
		}
	}
	for b in &node.blocks {
		match &b.body {
			BlkBody::Stmt(s) => collect_stmt_syms_deep(s, out),
			BlkBody::Cond(e) => collect_expr_syms_deep(e, out),
			BlkBody::Return(es) => {
				for e in es {
					collect_expr_syms_deep(e, out);
				}
			}
			BlkBody::Loop(n) => loop_node_refs(n, out),
			_ => {}
		}
	}
}

/// Shallow reference set of a block's own code (no function bodies).
fn block_own_refs(b: &Blk, out: &mut HashSet<SymId>) {
	match &b.body {
		BlkBody::Stmt(s) => collect_stmt_syms_shallow(s, out),
		BlkBody::Cond(e) => collect_expr_syms(e, out),
		BlkBody::Return(es) => {
			for e in es {
				collect_expr_syms(e, out);
			}
		}
		BlkBody::Loop(node) => loop_node_refs(node, out),
		_ => {}
	}
}

fn collect_stmt_syms_shallow(s: &Stmt, out: &mut HashSet<SymId>) {
	match s {
		Stmt::Local { values, .. } => {
			for v in values {
				if let Some(e) = v {
					collect_expr_syms(e, out);
				}
			}
		}
		Stmt::Assign { targets, values } => {
			for t in targets {
				collect_expr_syms(t, out);
			}
			for v in values {
				collect_expr_syms(v, out);
			}
		}
		Stmt::ExprStmt(e) => collect_expr_syms(e, out),
		_ => {}
	}
}

// ---------------------------------------------------------------------------
// machine emission
// ---------------------------------------------------------------------------

/// Build the state machine for one function body / chunk.
fn machine_block(
	block: &Block,
	table: &mut SymTable,
	rng: &mut Rng,
	reserved: &mut HashSet<String>,
) -> Block {
	let mut ctx = Ctx::new(false);
	let (start, exit) = build_graph(&mut ctx, &block.stmts, Cont::Join, table, rng, reserved);
	let n = ctx.blocks.len();
	let reach = reachable(&ctx.blocks, start);

	// ---- declarations & references -------------------------------------
	let mut decl: HashMap<SymId, usize> = HashMap::new();
	let mut refs: HashMap<SymId, HashSet<usize>> = HashMap::new();
	for (i, b) in ctx.blocks.iter().enumerate() {
		if !reach.contains(&i) {
			continue;
		}
		match &b.body {
			BlkBody::Stmt(Stmt::Local { syms, .. }) => {
				for &s in syms {
					decl.entry(s).or_insert(i);
				}
			}
			BlkBody::Stmt(Stmt::LocalFunc { sym, .. }) => {
				decl.entry(*sym).or_insert(i);
			}
			_ => {}
		}
		let mut syms = HashSet::new();
		block_own_refs(b, &mut syms);
		for s in syms {
			refs.entry(s).or_default().insert(i);
		}
		// closures / named functions defined IN this block: their bodies
		// capture from this block's scope, so any local they reference
		// must be visible from this arm (hoist if declared elsewhere)
		let mut fns = Vec::new();
		collect_functions_stmt(b, &mut fns);
		for f in fns {
			let mut syms2 = HashSet::new();
			for st in &f.stmts {
				collect_stmt_syms_deep(st, &mut syms2);
			}
			for s in syms2 {
				refs.entry(s).or_default().insert(i);
			}
		}
	}

	// hoist: referenced from a block other than the declaration block
	let mut hoisted: HashSet<SymId> = HashSet::new();
	for (s, d) in &decl {
		if let Some(rs) = refs.get(s) {
			for r in rs {
				if *r != *d {
					hoisted.insert(*s);
					break;
				}
			}
		}
	}
	// hoisting is per STATEMENT: a local statement with any cross-arm
	// reference moves wholesale to the machine scope (the declaration
	// becomes a plain assignment with the original value list intact —
	// keeping tail-call multi-value expansion). This keeps the transform
	// semantics-exact.
	let mut hoisted_stmts: HashSet<usize> = HashSet::new();
	for (s, d) in &decl {
		if hoisted.contains(s) {
			hoisted_stmts.insert(*d);
		}
	}

	// machine-top locals, in declaration order (whole hoisted statements)
	let mut top_locals: Vec<(SymId, String)> = Vec::new();
	{
		let mut seen = HashSet::new();
		for i in 0..n {
			if !reach.contains(&i) || !hoisted_stmts.contains(&i) {
				continue;
			}
			let pairs: Vec<(SymId, String)> = match &ctx.blocks[i].body {
				BlkBody::Stmt(Stmt::Local { names, syms, .. }) => names
					.iter()
					.zip(syms.iter())
					.map(|(nm, s)| (*s, nm.clone()))
					.collect(),
				BlkBody::Stmt(Stmt::LocalFunc { sym, .. }) => {
					vec![(*sym, table.name_of(*sym).to_string())]
				}
				_ => Vec::new(),
			};
			for (s, nm) in pairs {
				if seen.insert(s) {
					top_locals.push((s, nm));
				}
			}
		}
	}

	// fresh symbols: state var + one temp per reachable cond block
	let (st_sym, st_name) = new_rand_sym(table, rng, reserved);
	let mut temps: HashMap<usize, (SymId, String)> = HashMap::new();
	for i in 0..n {
		if reach.contains(&i) && matches!(ctx.blocks[i].body, BlkBody::Cond(_)) {
			let (s, nm) = new_rand_sym(table, rng, reserved);
			temps.insert(i, (s, nm));
		}
	}

	// random unique state IDs (shared id space for the whole function
	// machine, including nested sub-machines)
	let mut used: HashSet<u32> = HashSet::new();
	let mut ids: HashMap<usize, u32> = HashMap::new();
	for i in 0..n {
		if !reach.contains(&i) {
			continue;
		}
		loop {
			let id = rng.int(1000, 4_294_967_295) as u32;
			if used.insert(id) {
				ids.insert(i, id);
				break;
			}
		}
	}

	let st_ident = || Expr::Ident {
		name: st_name.clone(),
		sym: Some(st_sym),
	};
	let assign_st = |target: usize| Stmt::Assign {
		targets: vec![st_ident()],
		values: vec![num(ids[&target])],
	};

	// ---- per-block code --------------------------------------------------
	let mut branches: Vec<(usize, Vec<Stmt>)> = Vec::new();
	for i in 0..n {
		if !reach.contains(&i) {
			continue;
		}
		let code = match &ctx.blocks[i].body {
			BlkBody::Stmt(s) => {
				let mut v = Vec::new();
				match s {
					Stmt::LocalFunc { sym, func, .. }
						if hoisted_stmts.contains(&i) =>
					{
						// declaration moved to the machine top
						v.push(Stmt::Assign {
							targets: vec![Expr::Ident {
								name: table.name_of(*sym).to_string(),
								sym: Some(*sym),
							}],
							values: vec![Expr::Function {
								params: func.params.clone(),
								param_syms: func.param_syms.clone(),
								vararg: func.vararg,
								body: func.body.clone(),
							}],
						});
					}
					Stmt::Local { names, syms, values } if hoisted_stmts.contains(&i) => {
						// declaration moved to the machine top: the
						// statement becomes a plain assignment with the
						// original value list (multi-value tail expansion
						// preserved exactly)
						let mut vals: Vec<Expr> = values.iter().filter_map(|x| x.clone()).collect();
						if vals.is_empty() {
							vals.push(Expr::Nil);
						}
						v.push(Stmt::Assign {
							targets: names
								.iter()
								.zip(syms.iter())
								.map(|(nm, sy)| Expr::Ident {
									name: nm.clone(),
									sym: Some(*sy),
								})
								.collect(),
							values: vals,
						});
					}
					other => v.push(other.clone()),
				}
				if ctx.blocks[i].succ.len() == 1 {
					if let Succ::B(x) = ctx.blocks[i].succ[0] {
						v.push(assign_st(x));
					}
				}
				v
			}
			BlkBody::Return(es) => vec![Stmt::Return(es.clone())],
			BlkBody::Join => {
				if i == exit || ctx.blocks[i].succ.is_empty() {
					// the chunk/function end join
					vec![Stmt::Break]
				} else if let Succ::B(x) = ctx.blocks[i].succ[0] {
					vec![assign_st(x)]
				} else {
					vec![Stmt::Break]
				}
			}
			BlkBody::Cond(e) => {
				let succ = ctx.blocks[i].succ.clone();
				let (t, f) = (succ[0].clone(), succ[1].clone());
				let (tmp_sym, tmp_name) = temps[&i].clone();
				let tmp_ident = Expr::Ident {
					name: tmp_name.clone(),
					sym: Some(tmp_sym),
				};
				let tail = |su: Succ| -> Vec<Stmt> {
					match su {
						Succ::B(x) => vec![assign_st(x)],
						_ => vec![Stmt::Break],
					}
				};
				vec![
					Stmt::Local {
						names: vec![tmp_name],
						syms: vec![tmp_sym],
						values: vec![Some(e.clone())],
					},
					Stmt::If {
						cond: Box::new(tmp_ident),
						thenb: Block { stmts: tail(t) },
						elsifs: Vec::new(),
						elseb: Some(Block { stmts: tail(f) }),
					},
				]
			}
			BlkBody::Loop(node) => {
				let exit = match ctx.blocks[i].succ[0] {
					Succ::B(x) => x,
					_ => panic!("loop node must have a single successor"),
				};
				emit_loop(node, &st_ident(), ids[&i], ids[&exit], &mut used, table, rng, reserved)
			}
		};
		branches.push((i, code));
	}

	let dispatch = dispatch_chain(&branches, &ids, &st_ident, rng);

	let mut top = Vec::new();
	top.push(Stmt::Local {
		names: vec![st_name],
		syms: vec![st_sym],
		values: vec![Some(num(ids[&start]))],
	});
	if !top_locals.is_empty() {
		top.push(Stmt::Local {
			names: top_locals.iter().map(|(_, nm)| nm.clone()).collect(),
			syms: top_locals.iter().map(|(s, _)| *s).collect(),
			values: Vec::new(),
		});
	}
	top.push(Stmt::While {
		cond: Box::new(Expr::Bool { value: true }),
		body: Block {
			stmts: vec![dispatch],
		},
	});
	Block { stmts: top }
}

/// Shuffle `branches` and chain them into one if/elseif dispatch over the
/// given state variable.
fn dispatch_chain(
	branches: &[(usize, Vec<Stmt>)],
	ids: &HashMap<usize, u32>,
	st_ident: &dyn Fn() -> Expr,
	rng: &mut Rng,
) -> Stmt {
	let order: Vec<usize> = {
		let mut idx: Vec<usize> = (0..branches.len()).collect();
		rng.shuffle(&mut idx);
		idx
	};
	let mut chain_conds: Vec<Expr> = Vec::new();
	let mut chain_bodies: Vec<Block> = Vec::new();
	for &k in &order {
		let (i, code) = &branches[k];
		chain_conds.push(Expr::Bin {
			op: BinOp::Eq,
			l: Box::new(st_ident()),
			r: Box::new(num(ids[i])),
		});
		chain_bodies.push(Block { stmts: code.clone() });
	}
	let first = chain_bodies
		.first()
		.cloned()
		.expect("machine has at least one block");
	let elsifs: Vec<(Expr, Block)> = chain_conds[1..]
		.iter()
		.zip(chain_bodies[1..].iter())
		.map(|(c, b)| (c.clone(), b.clone()))
		.collect();
	Stmt::If {
		cond: Box::new(chain_conds[0].clone()),
		thenb: first,
		elsifs,
		elseb: None,
	}
}

/// Emit one loop node as its arm code: per-pass scope + nested dispatch +
/// bookkeeping. `state` is the enclosing dispatch's state var (st / st_l);
/// on re-pass the enclosing state is set to `reenter_id`, on loop exit to
/// `exit_id`.
fn emit_loop(
	node: &LoopNode,
	state: &Expr,
	reenter_id: u32,
	exit_id: u32,
	used: &mut HashSet<u32>,
	table: &mut SymTable,
	rng: &mut Rng,
	reserved: &mut HashSet<String>,
) -> Vec<Stmt> {
	let n = node.blocks.len();
	let reach = reachable(&node.blocks, node.start);

	// ids for the inner dispatch (same id space as the enclosing machine)
	let mut ids: HashMap<usize, u32> = HashMap::new();
	for i in 0..n {
		if !reach.contains(&i) {
			continue;
		}
		loop {
			let id = rng.int(1000, 4_294_967_295) as u32;
			if used.insert(id) {
				ids.insert(i, id);
				break;
			}
		}
	}

	// per-block cond temps
	let mut temps: HashMap<usize, (SymId, String)> = HashMap::new();
	for i in 0..n {
		if reach.contains(&i) && matches!(node.blocks[i].body, BlkBody::Cond(_)) {
			let (s, nm) = new_rand_sym(table, rng, reserved);
			temps.insert(i, (s, nm));
		}
	}

	// per-pass locals: every local declared anywhere in the body graph is
	// declared (nil) in the per-pass scope — fresh on every iteration,
	// visible to every body arm and to closures the body creates
	let mut perpass: Vec<(SymId, String)> = Vec::new();
	{
		let mut seen = HashSet::new();
		for i in 0..n {
			if !reach.contains(&i) {
				continue;
			}
			let pairs: Vec<(SymId, String)> = match &node.blocks[i].body {
				BlkBody::Stmt(Stmt::Local { names, syms, .. }) => names
					.iter()
					.zip(syms.iter())
					.map(|(nm, s)| (*s, nm.clone()))
					.collect(),
				BlkBody::Stmt(Stmt::LocalFunc { sym, .. }) => {
					vec![(*sym, table.name_of(*sym).to_string())]
				}
				_ => Vec::new(),
			};
			for (s, nm) in pairs {
				if seen.insert(s) {
					perpass.push((s, nm));
				}
			}
		}
	}

	// fresh per-pass symbols
	let (stl_sym, stl_name) = new_rand_sym(table, rng, reserved);
	let (brk_sym, brk_name) = new_rand_sym(table, rng, reserved);
	let stl_ident = || Expr::Ident {
		name: stl_name.clone(),
		sym: Some(stl_sym),
	};
	let brk_ident = || Expr::Ident {
		name: brk_name.clone(),
		sym: Some(brk_sym),
	};
	let set_state = |id: u32| Stmt::Assign {
		targets: vec![state.clone()],
		values: vec![num(id)],
	};

	// inner arms
	let mut branches: Vec<(usize, Vec<Stmt>)> = Vec::new();
	for i in 0..n {
		if !reach.contains(&i) {
			continue;
		}
		let code = match &node.blocks[i].body {
			BlkBody::Stmt(s) => {
				let mut v = Vec::new();
				match s {
					Stmt::Local { names, syms, values } => {
						// all body locals live in the per-pass scope: the
						// original declaration becomes a plain assignment
						// with the original value list (multi-value tail
						// expansion preserved exactly)
						let mut vals: Vec<Expr> = values.iter().filter_map(|x| x.clone()).collect();
						if vals.is_empty() {
							vals.push(Expr::Nil);
						}
						v.push(Stmt::Assign {
							targets: names
								.iter()
								.zip(syms.iter())
								.map(|(nm, sy)| Expr::Ident {
									name: nm.clone(),
									sym: Some(*sy),
								})
								.collect(),
							values: vals,
						});
					}
					Stmt::LocalFunc { sym, func, .. } => {
						v.push(Stmt::Assign {
							targets: vec![Expr::Ident {
								name: table.name_of(*sym).to_string(),
								sym: Some(*sym),
							}],
							values: vec![Expr::Function {
								params: func.params.clone(),
								param_syms: func.param_syms.clone(),
								vararg: func.vararg,
								body: func.body.clone(),
							}],
						});
					}
					other => v.push(other.clone()),
				}
				if node.blocks[i].succ.len() == 1 {
					match node.blocks[i].succ[0] {
						Succ::B(x) => v.push(Stmt::Assign {
							targets: vec![stl_ident()],
							values: vec![num(ids[&x])],
						}),
						Succ::Brk => {
							v.push(Stmt::Assign {
								targets: vec![brk_ident()],
								values: vec![Expr::Bool { value: true }],
							});
							v.push(Stmt::Break);
						}
						Succ::Cont => unreachable!("continue must be rewired"),
					}
				}
				v
			}
			BlkBody::Cond(e) => {
				let succ = node.blocks[i].succ.clone();
				let (t, f) = (succ[0].clone(), succ[1].clone());
				let (tmp_sym, tmp_name) = temps[&i].clone();
				let tmp_ident = Expr::Ident {
					name: tmp_name.clone(),
					sym: Some(tmp_sym),
				};
				let tail = |su: Succ| -> Vec<Stmt> {
					match su {
						Succ::B(x) => vec![Stmt::Assign {
							targets: vec![stl_ident()],
							values: vec![num(ids[&x])],
						}],
						Succ::Brk => vec![
							Stmt::Assign {
								targets: vec![brk_ident()],
								values: vec![Expr::Bool { value: true }],
							},
							Stmt::Break,
						],
						Succ::Cont => unreachable!("continue must be rewired"),
					}
				};
				vec![
					Stmt::Local {
						names: vec![tmp_name],
						syms: vec![tmp_sym],
						values: vec![Some(e.clone())],
					},
					Stmt::If {
						cond: Box::new(tmp_ident),
						thenb: Block { stmts: tail(t) },
						elsifs: Vec::new(),
						elseb: Some(Block { stmts: tail(f) }),
					},
				]
			}
			BlkBody::Join => {
				match node.blocks[i].succ.first() {
					None => {
						// the body-end join: normal pass completion
						vec![Stmt::Break]
					}
					Some(Succ::B(x)) => vec![Stmt::Assign {
						targets: vec![stl_ident()],
						values: vec![num(ids[x])],
					}],
					Some(Succ::Brk) => vec![
						Stmt::Assign {
							targets: vec![brk_ident()],
							values: vec![Expr::Bool { value: true }],
						},
						Stmt::Break,
					],
					Some(Succ::Cont) => unreachable!("continue must be rewired"),
				}
			}
			BlkBody::Return(es) => vec![Stmt::Return(es.clone())],
			BlkBody::Loop(n2) => {
				let exit = match node.blocks[i].succ[0] {
					Succ::B(x) => x,
					_ => panic!("loop node must have a single successor"),
				};
				let stl = stl_ident();
				emit_loop(n2, &stl, ids[&i], ids[&exit], used, table, rng, reserved)
			}
		};
		branches.push((i, code));
	}

	let dispatch = dispatch_chain(&branches, &ids, &stl_ident, rng);
	let dispatch_while = Stmt::While {
		cond: Box::new(Expr::Bool { value: true }),
		body: Block {
			stmts: vec![dispatch],
		},
	};

	let stl_init = Stmt::Local {
		names: vec![stl_name.clone()],
		syms: vec![stl_sym],
		values: vec![Some(num(ids[&node.start]))],
	};
	let brk_init = Stmt::Local {
		names: vec![brk_name.clone()],
		syms: vec![brk_sym],
		values: vec![Some(Expr::Bool { value: false })],
	};
	let perpass_decl = |syms: &[(SymId, String)]| -> Option<Stmt> {
		if syms.is_empty() {
			None
		} else {
			Some(Stmt::Local {
				names: syms.iter().map(|(_, nm)| nm.clone()).collect(),
				syms: syms.iter().map(|(s, _)| *s).collect(),
				values: Vec::new(),
			})
		}
	};
	let broke_check = || {
		Stmt::If {
			cond: Box::new(brk_ident()),
			thenb: Block {
				stmts: vec![set_state(exit_id)],
			},
			elsifs: Vec::new(),
			elseb: Some(Block { stmts: vec![set_state(reenter_id)] }),
		}
	};

	match &node.kind {
		LoopKind::ForNum { var, lim, stp, cur } => {
			let lim_i = ident(*lim, table);
			let stp_i = ident(*stp, table);
			let cur_i = ident(*cur, table);
			// range exceeded? (checked BEFORE the body each pass, like the
			// original for: an empty range runs 0 iterations)
			let exceeded = Expr::Bin {
				op: BinOp::Or,
				l: Box::new(Expr::Bin {
					op: BinOp::And,
					l: Box::new(Expr::Bin {
						op: BinOp::Ge,
						l: Box::new(stp_i.clone()),
						r: Box::new(num(0)),
					}),
					r: Box::new(Expr::Bin {
						op: BinOp::Gt,
						l: Box::new(cur_i.clone()),
						r: Box::new(lim_i.clone()),
					}),
				}),
				r: Box::new(Expr::Bin {
					op: BinOp::And,
					l: Box::new(Expr::Bin {
						op: BinOp::Lt,
						l: Box::new(stp_i.clone()),
						r: Box::new(num(0)),
					}),
					r: Box::new(Expr::Bin {
						op: BinOp::Lt,
						l: Box::new(cur_i.clone()),
						r: Box::new(lim_i.clone()),
					}),
				}),
			};
			let pass = Block {
				stmts: {
					let mut v = vec![Stmt::Local {
						// FRESH loop variable every iteration (real Lua
						// for semantics; closures capture it per-iteration)
						names: vec![table.name_of(*var).to_string()],
						syms: vec![*var],
						values: vec![Some(cur_i.clone())],
					}];
					if let Some(p) = perpass_decl(&perpass) {
						v.push(p);
					}
					v.push(stl_init);
					v.push(brk_init);
					v.push(dispatch_while);
					v.push(Stmt::If {
						cond: Box::new(brk_ident()),
						thenb: Block {
							stmts: vec![set_state(exit_id)],
						},
						elsifs: Vec::new(),
						elseb: Some(Block {
							stmts: vec![
								// increment after the body
								Stmt::Assign {
									targets: vec![cur_i.clone()],
									values: vec![Expr::Bin {
										op: BinOp::Add,
										l: Box::new(cur_i),
										r: Box::new(stp_i),
									}],
								},
								set_state(reenter_id),
							],
						}),
					});
					v
				},
			};
			vec![Stmt::If {
				cond: Box::new(exceeded),
				thenb: Block {
					stmts: vec![set_state(exit_id)],
				},
				elsifs: Vec::new(),
				elseb: Some(pass),
			}]
		}
		LoopKind::ForGen { vars, it, stt, ctl } => {
			let it_i = ident(*it, table);
			let stt_i = ident(*stt, table);
			let ctl_i = ident(*ctl, table);
			let call = Expr::Call {
				func: Box::new(it_i.clone()),
				args: vec![stt_i, ctl_i],
			};
			let v0 = ident(vars[0], table);
			let pass = Block {
				stmts: {
					let mut v = vec![Stmt::Assign {
						targets: vec![ident(*ctl, table)],
						values: vec![v0.clone()],
					}];
					if let Some(p) = perpass_decl(&perpass) {
						v.push(p);
					}
					v.push(stl_init);
					v.push(brk_init);
					v.push(dispatch_while);
					v.push(broke_check());
					v
				},
			};
			vec![
				Stmt::Local {
					// FRESH loop variables every iteration
					names: vars.iter().map(|s| table.name_of(*s).to_string()).collect(),
					syms: vars.clone(),
					values: vec![Some(call)],
				},
				Stmt::If {
					cond: Box::new(Expr::Bin {
						op: BinOp::Eq,
						l: Box::new(v0),
						r: Box::new(Expr::Nil),
					}),
					thenb: Block {
						stmts: vec![set_state(exit_id)],
					},
					elsifs: Vec::new(),
					elseb: Some(pass),
				},
			]
		}
		LoopKind::While { cond } => {
			let (t_sym, t_name) = new_rand_sym(table, rng, reserved);
			let pass = Block {
				stmts: {
					let mut v = Vec::new();
					if let Some(p) = perpass_decl(&perpass) {
						v.push(p);
					}
					v.push(stl_init);
					v.push(brk_init);
					v.push(dispatch_while);
					v.push(broke_check());
					v
				},
			};
			vec![
				Stmt::Local {
					names: vec![t_name],
					syms: vec![t_sym],
					values: vec![Some(cond.clone())],
				},
				Stmt::If {
					cond: Box::new(Expr::Ident {
						name: table.name_of(t_sym).to_string(),
						sym: Some(t_sym),
					}),
					thenb: pass,
					elsifs: Vec::new(),
					elseb: Some(Block {
						stmts: vec![set_state(exit_id)],
					}),
				},
			]
		}
		LoopKind::Repeat { .. } => {
			// the until-check lives inside the dispatch (a cond block whose
			// false-edge re-enters the body), so one "pass" may contain many
			// original iterations
			let mut v = Vec::new();
			if let Some(p) = perpass_decl(&perpass) {
				v.push(p);
			}
			v.push(stl_init);
			v.push(brk_init);
			v.push(dispatch_while);
			v.push(broke_check());
			v
		}
	}
}

/// Find all function bodies (anonymous closures and local functions)
/// defined within a block's own statement — used to know which locals a
/// closure created in this arm captures.
fn collect_functions_stmt<'a>(b: &'a Blk, out: &mut Vec<&'a Block>) {
	match &b.body {
		BlkBody::Stmt(s) => collect_functions_stmt2(s, out),
		BlkBody::Cond(e) => collect_functions_expr(e, out),
		BlkBody::Return(es) => {
			for e in es {
				collect_functions_expr(e, out);
			}
		}
		_ => {}
	}
}

fn collect_functions_stmt2<'a>(s: &'a Stmt, out: &mut Vec<&'a Block>) {
	match s {
		Stmt::Local { values, .. } => {
			for v in values {
				if let Some(e) = v {
					collect_functions_expr(e, out);
				}
			}
		}
		Stmt::LocalFunc { func, .. } => {
			out.push(&func.body);
			collect_block_functions(&func.body, out);
		}
		Stmt::FuncDecl { func, obj, .. } => {
			if let Some(o) = obj {
				collect_functions_expr(o, out);
			}
			out.push(&func.body);
			collect_block_functions(&func.body, out);
		}
		Stmt::Assign { targets, values } => {
			for t in targets {
				collect_functions_expr(t, out);
			}
			for v in values {
				collect_functions_expr(v, out);
			}
		}
		Stmt::ExprStmt(e) => collect_functions_expr(e, out),
		Stmt::Return(es) => {
			for e in es {
				collect_functions_expr(e, out);
			}
		}
		Stmt::If { cond, thenb, elsifs, elseb } => {
			collect_functions_expr(cond, out);
			collect_block_functions(thenb, out);
			for (_, b) in elsifs {
				collect_block_functions(b, out);
			}
			if let Some(b) = elseb {
				collect_block_functions(b, out);
			}
		}
		Stmt::Do(b) => collect_block_functions(b, out),
		_ => {}
	}
}

fn collect_block_functions<'a>(b: &'a Block, out: &mut Vec<&'a Block>) {
	for s in &b.stmts {
		collect_functions_stmt2(s, out);
	}
}

fn collect_functions_expr<'a>(e: &'a Expr, out: &mut Vec<&'a Block>) {
	match e {
		Expr::Function { body, .. } => {
			out.push(body);
			collect_block_functions(body, out);
		}
		Expr::Dot { obj, .. } => collect_functions_expr(obj, out),
		Expr::Index { obj, idx } => {
			collect_functions_expr(obj, out);
			collect_functions_expr(idx, out);
		}
		Expr::Call { func, args } => {
			collect_functions_expr(func, out);
			for a in args {
				collect_functions_expr(a, out);
			}
		}
		Expr::Method { obj, args, .. } => {
			collect_functions_expr(obj, out);
			for a in args {
				collect_functions_expr(a, out);
			}
		}
		Expr::Bin { l, r, .. } => {
			collect_functions_expr(l, out);
			collect_functions_expr(r, out);
		}
		Expr::Un { e, .. } => collect_functions_expr(e, out),
		Expr::Table { fields } => {
			for f in fields {
				match f {
					TableField::Array(e) => collect_functions_expr(e, out),
					TableField::Key { key, value } => {
						collect_functions_expr(key, out);
						collect_functions_expr(value, out);
					}
				}
			}
		}
		_ => {}
	}
}

// ---------------------------------------------------------------------------
// entry point
// ---------------------------------------------------------------------------

/// Flatten every nested function body, then the given block itself.
pub fn flatten_block(
	block: &mut Block,
	table: &mut SymTable,
	rng: &mut Rng,
	reserved: &mut HashSet<String>,
) {
	flatten_functions(block, table, rng, reserved);
	*block = machine_block(block, table, rng, reserved);
}

fn flatten_functions(
	block: &mut Block,
	table: &mut SymTable,
	rng: &mut Rng,
	reserved: &mut HashSet<String>,
) {
	for s in block.stmts.iter_mut() {
		match s {
			Stmt::LocalFunc { func, .. } => flatten_block(&mut func.body, table, rng, reserved),
			Stmt::FuncDecl { func, .. } => flatten_block(&mut func.body, table, rng, reserved),
			Stmt::Local { values, .. } => {
				for v in values.iter_mut() {
					if let Some(e) = v {
						flatten_expr(e, table, rng, reserved);
					}
				}
			}
			Stmt::Assign { values, .. } => {
				for v in values.iter_mut() {
					flatten_expr(v, table, rng, reserved);
				}
			}
			Stmt::ExprStmt(e) => flatten_expr(e, table, rng, reserved),
			Stmt::Return(es) => {
				for e in es.iter_mut() {
					flatten_expr(e, table, rng, reserved);
				}
			}
			_ => {}
		}
	}
}

fn flatten_expr(
	e: &mut Expr,
	table: &mut SymTable,
	rng: &mut Rng,
	reserved: &mut HashSet<String>,
) {
	match e {
		Expr::Function { body, .. } => flatten_block(body, table, rng, reserved),
		Expr::Call { func, args } => {
			flatten_expr(func, table, rng, reserved);
			for a in args.iter_mut() {
				flatten_expr(a, table, rng, reserved);
			}
		}
		Expr::Method { obj, args, .. } => {
			flatten_expr(obj, table, rng, reserved);
			for a in args.iter_mut() {
				flatten_expr(a, table, rng, reserved);
			}
		}
		Expr::Bin { l, r, .. } => {
			flatten_expr(l, table, rng, reserved);
			flatten_expr(r, table, rng, reserved);
		}
		Expr::Un { e, .. } => flatten_expr(e, table, rng, reserved),
		Expr::Table { fields } => {
			for f in fields.iter_mut() {
				match f {
					TableField::Array(e) => flatten_expr(e, table, rng, reserved),
					TableField::Key { key, value } => {
						flatten_expr(key, table, rng, reserved);
						flatten_expr(value, table, rng, reserved);
					}
				}
			}
		}
		Expr::Dot { obj, .. } => flatten_expr(obj, table, rng, reserved),
		Expr::Index { obj, idx } => {
			flatten_expr(obj, table, rng, reserved);
			flatten_expr(idx, table, rng, reserved);
		}
		_ => {}
	}
}
