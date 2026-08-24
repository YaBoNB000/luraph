//! L6 — custom VM: the core moat.
//!
//! The user program is compiled to private bytecode (register VM,
//! per-build opcode permutation) which is executed by an interpreter
//! generated at build time — the interpreter itself is just Lua source
//! that runs through the full obfuscation pipeline, so the output is
//! [obfuscated interpreter] + [encrypted bytecode] with no natively
//! readable program structure.

pub mod compiler;
pub mod isa;
pub mod template;

pub use compiler::compile;
