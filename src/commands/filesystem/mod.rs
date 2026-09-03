//! Filesystem commands: `mkdir`, `cat`, `cp`, `mv`, `rm`, and `ls`.
//!
//! Each command lives in its own module and is re-exported here so the
//! dispatcher in `crate::commands` sees one flat namespace.

mod cat;
mod cp;
mod ls;
mod mkdir;
mod mv;
mod rm;

pub use cat::cat;
pub use cp::cp;
pub use ls::ls;
pub use mkdir::mkdir;
pub use mv::mv;
pub use rm::rm;
