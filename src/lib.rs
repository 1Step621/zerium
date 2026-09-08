//! Zerium application library.
//!
//! The binary is intentionally only a process entry point. The application is
//! not a reusable SDK yet, so its internal layers stay private while the
//! project is under active development.

#![deny(unreachable_pub)]

mod app;
mod domain;
mod engine;
mod plugin_catalog;
mod ui;

pub use app::run;
