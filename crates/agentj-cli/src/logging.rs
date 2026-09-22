//! Process logging initialisation: an `EnvFilter` that honours `RUST_LOG`,
//! keeping the historical `info` default when the variable is unset.

use tracing_subscriber::EnvFilter;

/// Build the process-wide log filter from a `RUST_LOG` spec.
pub fn filter_from(spec: Option<&str>) -> EnvFilter {
    match spec {
        Some(s) => EnvFilter::new(s),
        None => EnvFilter::new("info"),
    }
}

/// Install the global tracing subscriber, reading `RUST_LOG` (default `info`).
pub fn init() {
    let spec = std::env::var("RUST_LOG").ok();
    tracing_subscriber::fmt()
        .with_env_filter(filter_from(spec.as_deref()))
        .init();
}

#[cfg(test)]
mod tests {
    #[test]
    fn filter_defaults_to_info_and_honors_debug_spec() {
        assert_eq!(
            super::filter_from(None).max_level_hint(),
            Some(tracing::level_filters::LevelFilter::INFO)
        );
        assert_eq!(
            super::filter_from(Some("debug")).max_level_hint(),
            Some(tracing::level_filters::LevelFilter::DEBUG)
        );
    }
}
