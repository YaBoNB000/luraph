//! L5 whole-program encryption: after all other passes, the entire
//! program is printed, encrypted with a fresh per-build key (same
//! additive keystream as L2 — 5.1-safe, shared floor `%` semantics),
//! and replaced by a container:
//!
//!   local K1, K2, K3 = "<key part 1>", "<key part 2>", "<key part 3>"
//!   local KC = K1 .. K2 .. K3
//!   local KT = { ... }                       -- key byte table
//!   local function DEC(...) ... end          -- runtime decoder
//!   local C1 = "<cipher 1>"                  -- ciphertext chunks
//!   local C2 = "<cipher 2>"
//!   ...
//!   loadstring(DEC(C1, C2, C3))()
//!
//! No program text is visible in the output — only ciphertext strings.
//! (The L7 antidbg post-pass wraps the final loadstring call with
//! container checksum + timing trap + error rewrite.)

use crate::ast::*;
use crate::mangle::{gen_name, reserved_set};
use crate::rng::Rng;
use crate::strings::{build_loader, encrypt, split_into_parts, KEY_LEN};
use crate::symtab::{Sym, SymTable};
use std::collections::HashSet;

pub fn apply_body(
	block: &mut Block,
	table: &mut SymTable,
	rng: &mut Rng,
	reserved: &mut HashSet<String>,
	luau: bool,
) {
	// 1. serialize the current program (compact form for the payload)
	let text = crate::printer::print_chunk(table, block);
	let compact = crate::minify::minify(&text, luau).unwrap_or(text);

	// 2. encrypt the whole text with a fresh key
	let key: Vec<u8> = (0..KEY_LEN).map(|_| rng.int(0, 255) as u8).collect();
	let ct = encrypt(compact.as_bytes(), &key);

	// 3. split the ciphertext into a few chunk literals
	let n_chunks = (rng.int(3, 5) as usize).min(ct.len().max(1));
	let chunks = split_into_parts(&ct, n_chunks);

	// 4. decoder loader (reuses the L2 loader: key split + byte table +
	//    DEC vararg-concat decrypt function)
	let mut rsv = reserved_set(reserved);
	rsv.extend(table.globals.iter().cloned());
	rsv.extend(table.syms.iter().map(|s| s.name.clone()));
	// the container must never shadow the global loader
	rsv.insert("loadstring".to_string());
	let (loader, dec_sym, dec_name) = build_loader(table, rng, &mut rsv, &key);

	// 5. ciphertext chunk locals: local C1 = "<...>" ...
	let mut chunk_syms: Vec<SymId> = Vec::new();
	let mut chunk_stmts: Vec<Stmt> = Vec::new();
	for bytes in &chunks {
		let id = table.syms.len() as SymId;
		let name = gen_name(rng, &mut rsv);
		rsv.insert(name.clone());
		table.syms.push(Sym {
			name: name.clone(),
			is_param: false,
			keep_name: false,
		});
		chunk_syms.push(id);
		chunk_stmts.push(Stmt::Local {
			names: vec![name],
			syms: vec![id],
			values: vec![Some(Expr::Str {
				bytes: bytes.clone(),
			})],
		});
	}

	// 6. final statement: loadstring(DEC(C1, C2, C3))()
	//    (loadstring is the only runtime loader in BOTH dialects —
	//    Luau has no global `load`, verified in M0 research)
	let dec_call = Expr::Call {
		func: Box::new(Expr::Ident {
			name: dec_name,
			sym: Some(dec_sym),
		}),
		args: chunk_syms
			.iter()
			.map(|s| Expr::Ident {
				name: table.name_of(*s).to_string(),
				sym: Some(*s),
			})
			.collect(),
	};
	let final_call = Expr::Call {
		// ( loadstring( DEC(C1, C2, C3) ) )()
		func: Box::new(Expr::Call {
			func: Box::new(Expr::Ident {
				name: "loadstring".to_string(),
				sym: None,
			}),
			args: vec![dec_call],
		}),
		args: Vec::new(),
	};

	let mut stmts = Vec::new();
	stmts.extend(loader);
	stmts.extend(chunk_stmts);
	stmts.push(Stmt::ExprStmt(final_call));
	block.stmts = stmts;
}
