# deadpool-oracle

Async connection pool for Oracle databases using `oracle-rs` and `deadpool`.

[![Crates.io](https://img.shields.io/crates/v/deadpool-oracle.svg)](https://crates.io/crates/deadpool-oracle)
[![Documentation](https://docs.rs/deadpool-oracle/badge.svg)](https://docs.rs/deadpool-oracle)
[![Build Status](https://github.com/stiang/deadpool-oracle/actions/workflows/rust.yml/badge.svg)](https://github.com/stiang/deadpool-oracle/actions/workflows/rust.yml)
[![License](https://img.shields.io/crates/l/deadpool-oracle.svg)](LICENSE-APACHE)

## Features

- **Async connection pooling** - Built on the `deadpool` async pool library
- **Automatic connection health checks** - Connections are verified before reuse
- **Configurable pool size and timeouts** - Tune for your workload
- **Automatic cleanup** - Pending transactions are rolled back on connection return

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
oracle-rs = "0.1"
deadpool-oracle = "0.1"
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

Basic usage:

```rust
use oracle_rs::Config;
use deadpool_oracle::{Pool, PoolBuilder};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create connection config
    let config = Config::new("localhost", 1521, "FREEPDB1", "user", "password");

    // Create pool with default settings
    let pool = PoolBuilder::new(config)
        .max_size(10)
        .build()?;

    // Get a connection from the pool
    let conn = pool.get().await?;

    // Use the connection
    let result = conn.query("SELECT * FROM users", &[]).await?;
    println!("Found {} rows", result.row_count());

    // Connection is automatically returned to the pool when dropped
    Ok(())
}
```

## Pool Configuration

```rust
use oracle_rs::Config;
use deadpool_oracle::PoolBuilder;
use std::time::Duration;

let config = Config::new("localhost", 1521, "FREEPDB1", "user", "password");

let pool = PoolBuilder::new(config)
    // Maximum number of connections (default: num_cpus * 4)
    .max_size(20)
    // Timeout waiting for a connection from pool (default: 30s)
    .wait_timeout(Some(Duration::from_secs(60)))
    // Timeout for creating new connections (default: 30s)
    .create_timeout(Some(Duration::from_secs(30)))
    // Timeout for health checks on recycled connections (default: 5s)
    .recycle_timeout(Some(Duration::from_secs(5)))
    .build()?;
```

## Extension Trait

For convenience, you can create pools directly from a `Config`:

```rust
use oracle_rs::Config;
use deadpool_oracle::ConfigExt;

let config = Config::new("localhost", 1521, "FREEPDB1", "user", "password");

// Create pool with default settings
let pool = config.into_pool()?;

// Or with custom max size
let pool = config.into_pool_with_size(20)?;
```

## Pool Status

```rust
let status = pool.status();
println!("Pool size: {}", status.size);
println!("Available connections: {}", status.available);
println!("Waiting tasks: {}", status.waiting);
```

## Connection Lifecycle

When a connection is returned to the pool (dropped), the following happens:

1. Any pending transaction is rolled back
2. The connection is verified with a ping
3. If healthy, the connection is returned to the pool
4. If unhealthy, the connection is discarded

This ensures that each connection from the pool is in a clean, working state.

## With TLS/SSL

```rust
use oracle_rs::Config;
use deadpool_oracle::PoolBuilder;

let config = Config::new("hostname", 2484, "service_name", "user", "password")
    .with_tls()?;

let pool = PoolBuilder::new(config)
    .max_size(10)
    .build()?;
```

## With DRCP (Database Resident Connection Pooling)

For maximum efficiency with Oracle DRCP:

```rust
use oracle_rs::Config;
use deadpool_oracle::PoolBuilder;

let config = Config::new("hostname", 1521, "service_name", "user", "password")
    .with_drcp("my_app_pool", "self");

// Client-side pool works with server-side DRCP
let pool = PoolBuilder::new(config)
    .max_size(50)  // Can be larger since DRCP handles server-side pooling
    .build()?;
```

## Author

[Stian Grytøyr](https://github.com/stiang)

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Related

- [oracle-rs](https://github.com/oracle-rs/oracle-rs) - The pure Rust Oracle driver this pool is built for
- [deadpool](https://github.com/bikeshedder/deadpool) - The async pool library used under the hood
