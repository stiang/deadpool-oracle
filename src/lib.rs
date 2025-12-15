//! Deadpool connection pool for Oracle databases
//!
//! This crate provides a connection pool for the `oracle-rs` driver using the
//! `deadpool` async pool library.
//!
//! # Example
//!
//! ```rust,no_run
//! use oracle_rs::Config;
//! use deadpool_oracle::{Pool, PoolBuilder};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Create connection config
//!     let config = Config::new("localhost", 1521, "FREEPDB1", "user", "password");
//!
//!     // Create pool
//!     let pool = PoolBuilder::new(config)
//!         .max_size(10)
//!         .build()?;
//!
//!     // Get a connection from the pool
//!     let conn = pool.get().await?;
//!
//!     // Use the connection
//!     let result = conn.query("SELECT * FROM users", &[]).await?;
//!     println!("Found {} rows", result.row_count());
//!
//!     // Connection is automatically returned to the pool when dropped
//!     Ok(())
//! }
//! ```

use deadpool::managed::{self, Manager, Metrics, RecycleError, RecycleResult};
use oracle_rs::{Config, Connection, Error};
use std::time::Duration;

/// Manager for creating and recycling Oracle connections
///
/// This implements the `deadpool::managed::Manager` trait to integrate
/// with the deadpool connection pool.
pub struct OracleConnectionManager {
    config: Config,
}

impl OracleConnectionManager {
    /// Create a new connection manager with the given configuration
    pub fn new(config: Config) -> Self {
        Self { config }
    }
}

impl Manager for OracleConnectionManager {
    type Type = Connection;
    type Error = Error;

    async fn create(&self) -> Result<Connection, Error> {
        Connection::connect_with_config(self.config.clone()).await
    }

    async fn recycle(
        &self,
        conn: &mut Connection,
        _metrics: &Metrics,
    ) -> RecycleResult<Error> {
        // Check if connection is still alive
        if conn.is_closed() {
            return Err(RecycleError::message("connection closed"));
        }

        // Rollback any pending transaction to ensure clean state
        conn.rollback().await.ok();

        // Verify connection still works
        conn.ping().await.map_err(RecycleError::Backend)?;

        Ok(())
    }
}

/// Type alias for the connection pool
pub type Pool = managed::Pool<OracleConnectionManager>;

/// Type alias for a pooled connection
///
/// This wraps a `Connection` and automatically returns it to the pool when dropped.
pub type Object = managed::Object<OracleConnectionManager>;

/// Builder for creating connection pools with custom configuration
///
/// # Example
///
/// ```rust,no_run
/// use oracle_rs::Config;
/// use deadpool_oracle::PoolBuilder;
/// use std::time::Duration;
///
/// let config = Config::new("localhost", 1521, "FREEPDB1", "user", "password");
/// let pool = PoolBuilder::new(config)
///     .max_size(20)
///     .wait_timeout(Some(Duration::from_secs(30)))
///     .build()
///     .expect("Failed to build pool");
/// ```
pub struct PoolBuilder {
    config: Config,
    max_size: usize,
    wait_timeout: Option<Duration>,
    create_timeout: Option<Duration>,
    recycle_timeout: Option<Duration>,
}

impl PoolBuilder {
    /// Create a new pool builder with the given connection configuration
    pub fn new(config: Config) -> Self {
        Self {
            config,
            max_size: num_cpus() * 4,
            wait_timeout: Some(Duration::from_secs(30)),
            create_timeout: Some(Duration::from_secs(30)),
            recycle_timeout: Some(Duration::from_secs(5)),
        }
    }

    /// Set the maximum number of connections in the pool
    ///
    /// Default is `num_cpus * 4`.
    pub fn max_size(mut self, size: usize) -> Self {
        self.max_size = size;
        self
    }

    /// Set the timeout for waiting for a connection from the pool
    ///
    /// If the pool is exhausted, this is how long to wait before returning an error.
    /// Default is 30 seconds. Set to `None` to wait indefinitely.
    pub fn wait_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.wait_timeout = timeout;
        self
    }

    /// Set the timeout for creating a new connection
    ///
    /// Default is 30 seconds. Set to `None` to wait indefinitely.
    pub fn create_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.create_timeout = timeout;
        self
    }

    /// Set the timeout for recycling a connection (health check)
    ///
    /// Default is 5 seconds. Set to `None` to wait indefinitely.
    pub fn recycle_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.recycle_timeout = timeout;
        self
    }

    /// Build the connection pool
    ///
    /// This creates the pool but does not establish any connections.
    /// Connections are created lazily when first requested.
    pub fn build(self) -> Result<Pool, BuildError> {
        let manager = OracleConnectionManager::new(self.config);

        let builder = managed::Pool::builder(manager)
            .max_size(self.max_size)
            .runtime(deadpool::Runtime::Tokio1)
            .timeouts(managed::Timeouts {
                wait: self.wait_timeout,
                create: self.create_timeout,
                recycle: self.recycle_timeout,
            });

        builder.build().map_err(BuildError)
    }
}

/// Error that can occur when building a connection pool
#[derive(Debug)]
pub struct BuildError(managed::BuildError);

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "failed to build connection pool: {}", self.0)
    }
}

impl std::error::Error for BuildError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

/// Error that can occur when getting a connection from the pool
pub type PoolError = managed::PoolError<Error>;

/// Helper to get CPU count for default pool size
fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|p| p.get())
        .unwrap_or(4)
}

/// Extension trait for creating pools directly from Config
pub trait ConfigExt {
    /// Create a connection pool with default configuration
    fn into_pool(self) -> Result<Pool, BuildError>;

    /// Create a connection pool with the specified maximum size
    fn into_pool_with_size(self, max_size: usize) -> Result<Pool, BuildError>;
}

impl ConfigExt for Config {
    fn into_pool(self) -> Result<Pool, BuildError> {
        PoolBuilder::new(self).build()
    }

    fn into_pool_with_size(self, max_size: usize) -> Result<Pool, BuildError> {
        PoolBuilder::new(self).max_size(max_size).build()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pool_builder_defaults() {
        let config = Config::new("localhost", 1521, "FREEPDB1", "test", "test");
        let builder = PoolBuilder::new(config);

        assert!(builder.max_size > 0);
        assert!(builder.wait_timeout.is_some());
        assert!(builder.create_timeout.is_some());
        assert!(builder.recycle_timeout.is_some());
    }

    #[test]
    fn test_pool_builder_configuration() {
        let config = Config::new("localhost", 1521, "FREEPDB1", "test", "test");
        let builder = PoolBuilder::new(config)
            .max_size(5)
            .wait_timeout(Some(Duration::from_secs(10)))
            .create_timeout(None)
            .recycle_timeout(Some(Duration::from_secs(2)));

        assert_eq!(builder.max_size, 5);
        assert_eq!(builder.wait_timeout, Some(Duration::from_secs(10)));
        assert_eq!(builder.create_timeout, None);
        assert_eq!(builder.recycle_timeout, Some(Duration::from_secs(2)));
    }

    #[test]
    fn test_pool_build_lazy() {
        let config = Config::new("localhost", 1521, "FREEPDB1", "test", "test");
        let pool = PoolBuilder::new(config).max_size(10).build();

        // Pool creation should succeed (connections are lazy)
        assert!(pool.is_ok());

        let pool = pool.unwrap();
        let status = pool.status();

        // No connections yet (lazy)
        assert_eq!(status.size, 0);
        assert_eq!(status.available, 0);
    }
}
