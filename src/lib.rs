//! Docker
#![doc(html_root_url = "https://ghmlee.github.io/rust-docker/doc")]
// Increase the compiler's recursion limit for the `error_chain` crate.
#![recursion_limit = "1024"]

// import external libraries
#[macro_use]
extern crate error_chain;

#[cfg(windows)]
extern crate named_pipe;


#[macro_use]
extern crate serde_derive;




// declare modules
pub mod container;
mod docker;
pub mod errors;
pub mod filesystem;
pub mod image;
mod options;
pub mod process;
pub mod stats;
pub mod system;
mod test;
#[cfg(unix)]
mod unix;
mod util;
pub mod version;

// publicly re-export
pub use crate::docker::Docker;
pub use crate::options::*;
