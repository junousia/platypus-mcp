pub mod approvals;
pub mod assignments;
pub mod backlog;
pub mod bundle;
pub mod cli;
pub mod config;
pub mod dispatch;
pub mod events;
pub mod evidence;
pub mod findings;
pub mod guidance;
pub mod host_guidance;
pub mod integrations;
pub mod leases;
pub mod models;
pub mod project;
pub mod reconcile;
pub mod runner;
pub mod server;
pub mod state;
pub mod storage;
pub mod tasks;
pub mod workers;
pub mod workspace;

mod git_trailers;

pub use server::{serve_stdio, PlatypusMcp};
