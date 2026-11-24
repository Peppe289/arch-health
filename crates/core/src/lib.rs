//! arch-health core: domain model, ports, checks and runner.
//!
//! This crate has no system dependencies (see ADR-0003): everything that
//! touches the system goes through the traits in [`ports`].

// Tests build a default fake and then set the fields they care about.
#![cfg_attr(test, allow(clippy::field_reassign_with_default))]

pub mod checks;
pub mod config;
pub mod error;
pub mod model;
pub mod ports;
pub mod runner;

#[cfg(test)]
pub(crate) mod fake;

pub use config::Config;
pub use error::{ErrorKind, PortError, PortResult};
pub use model::{Category, CheckId, CheckReport, Gauge, Item, Report, Status, TimedEvent};
pub use ports::Ports;
pub use runner::{Event, run};
