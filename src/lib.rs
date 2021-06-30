#![warn(rust_2018_idioms)]
#![allow(unused_macros)]
#![allow(clippy::missing_safety_doc)]

#[macro_use]
mod macros;

#[macro_use]
extern crate log;

pub mod arch;
pub mod consts;
pub mod debug_manager;
pub mod error;
pub mod gdb_parser;
#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
pub mod paging;
#[cfg(target_os = "linux")]
pub mod shared_queue;
pub mod utils;
pub mod vm;

pub use arch::*;
use core_affinity::CoreId;
use std::hint;
use std::path::PathBuf;
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::thread;
use vm::Vm;

/// Creates a uhyve vm and runs the binary given by `path` in it.
/// Blocks until the VM has finished execution.
pub fn uhyve_run(
	path: PathBuf,
	vm_params: &vm::Parameter<'_>,
