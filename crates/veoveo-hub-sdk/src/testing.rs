//! Checks a server runs as ordinary tests inside its Docker check.
//!
//! [`assert_sql_confined`] fails a server whose SQL names any schema but its own. A
//! server reaches core state, such as its Tasks, usage and audit, only through this SDK
//! and core's runtime API. [`knowledge`] checks, over a server's own fixture, that the
//! server expresses the knowledge contract.
#[cfg(feature = "server")]
mod knowledge;
mod sql_confinement;

#[cfg(feature = "server")]
pub use knowledge::{SYNTHETIC_SIGNATURE, knowledge, synthetic_media};
pub use sql_confinement::{SqlFinding, SqlProblem, assert_sql_confined, check_sql_confined};
