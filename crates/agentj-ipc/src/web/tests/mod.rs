//! Web surface tests, grouped by the domain module they exercise. Shared
//! fixtures live in [`support`]; each domain file pulls the production items it
//! needs straight from its own module.

mod auth;
mod config;
mod endpoints;
mod integrity;
mod persist;
mod prometheus;
mod router;
mod structure;
mod support;
