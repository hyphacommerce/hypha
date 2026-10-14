use std::time::Duration;

/// How long to wait for Postgres to accept a connection before giving up. Without it a
/// stopped database hangs a host's startup, or a test run, for as long as the OS waits.
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Parses a `postgres://` URL into a connection config with `DEFAULT_CONNECT_TIMEOUT`,
/// unless the URL sets its own (`?connect_timeout=10`). Opening the connection is up to
/// the caller (`Config::connect`, a pool), which keeps this crate runtime-free.
pub fn database_config(url: &str) -> Result<tokio_postgres::Config, tokio_postgres::Error> {
    let mut config: tokio_postgres::Config = url.parse()?;
    if config.get_connect_timeout().is_none() {
        config.connect_timeout(DEFAULT_CONNECT_TIMEOUT);
    }
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sets_the_default_timeout_unless_the_url_has_one() {
        let url = "postgres://hypha:hypha@localhost:5432/hypha";
        assert_eq!(
            database_config(url).unwrap().get_connect_timeout(),
            Some(&DEFAULT_CONNECT_TIMEOUT)
        );
        assert_eq!(
            database_config(&format!("{url}?connect_timeout=12"))
                .unwrap()
                .get_connect_timeout(),
            Some(&Duration::from_secs(12))
        );
        assert!(database_config("not a url").is_err());
    }
}
