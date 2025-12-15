//! Integration tests for deadpool-oracle connection pooling
//!
//! These tests require a running Oracle database. Set the ORACLE_TEST_URL
//! environment variable to run them.

use deadpool_oracle::{ConfigExt, Pool, PoolBuilder};
use oracle_rs::Config;
use std::time::Duration;

fn get_test_config() -> Option<Config> {
    // Use same environment variables as oracle-rs integration tests
    let host = std::env::var("ORACLE_HOST").unwrap_or_else(|_| "localhost".to_string());
    let port: u16 = std::env::var("ORACLE_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(1521);
    let service = std::env::var("ORACLE_SERVICE").unwrap_or_else(|_| "FREEPDB1".to_string());
    let username = std::env::var("ORACLE_USER").unwrap_or_else(|_| "system".to_string());
    let password = std::env::var("ORACLE_PASSWORD").unwrap_or_else(|_| "testpass".to_string());

    // Check if ORACLE_TEST_URL is set to override
    if std::env::var("ORACLE_TEST_URL").is_err() {
        return None;
    }

    let mut config = Config::new(&host, port, &service, &username, &password);
    config.set_password(&password);
    Some(config)
}

#[tokio::test]
#[ignore = "requires Oracle database"]
async fn test_pool_basic() {
    let config = get_test_config().expect("ORACLE_TEST_URL not set");

    // Create pool with small size for testing
    let pool = PoolBuilder::new(config)
        .max_size(2)
        .build()
        .expect("Failed to build pool");

    // Get a connection
    let conn = pool.get().await.expect("Failed to get connection");

    // Use the connection
    let result = conn
        .query("SELECT 1 FROM DUAL", &[])
        .await
        .expect("Query failed");

    assert_eq!(result.row_count(), 1);

    // Check pool status
    let status = pool.status();
    println!("Pool status: size={}, available={}", status.size, status.available);
    assert_eq!(status.size, 1); // One connection created
    assert_eq!(status.available, 0); // It's currently in use

    // Drop connection - should return to pool
    drop(conn);

    // Check status again
    let status = pool.status();
    assert_eq!(status.available, 1); // Now available
}

#[tokio::test]
#[ignore = "requires Oracle database"]
async fn test_pool_reuse() {
    let config = get_test_config().expect("ORACLE_TEST_URL not set");

    let pool = PoolBuilder::new(config)
        .max_size(1)
        .build()
        .expect("Failed to build pool");

    // Get and use connection
    {
        let conn = pool.get().await.expect("Failed to get connection");
        conn.query("SELECT 1 FROM DUAL", &[]).await.expect("Query failed");
    }

    // Get connection again - should reuse the same one
    {
        let conn = pool.get().await.expect("Failed to get connection");
        conn.query("SELECT 2 FROM DUAL", &[]).await.expect("Query failed");
    }

    // Only one connection should have been created
    let status = pool.status();
    assert_eq!(status.size, 1);
    assert_eq!(status.available, 1);
}

#[tokio::test]
#[ignore = "requires Oracle database"]
async fn test_pool_concurrent() {
    let config = get_test_config().expect("ORACLE_TEST_URL not set");

    let pool = PoolBuilder::new(config)
        .max_size(3)
        .build()
        .expect("Failed to build pool");

    // Spawn multiple tasks using the pool
    let mut handles = vec![];

    for i in 0..5 {
        let pool = pool.clone();
        let handle = tokio::spawn(async move {
            let conn = pool.get().await.expect("Failed to get connection");
            let result = conn
                .query(&format!("SELECT {} FROM DUAL", i), &[])
                .await
                .expect("Query failed");
            assert_eq!(result.row_count(), 1);
            // Small delay to ensure overlap
            tokio::time::sleep(Duration::from_millis(50)).await;
        });
        handles.push(handle);
    }

    // Wait for all tasks
    for handle in handles {
        handle.await.expect("Task panicked");
    }

    // Should have created at most max_size connections
    let status = pool.status();
    assert!(status.size <= 3);
    println!("Final pool size: {}", status.size);
}

#[tokio::test]
#[ignore = "requires Oracle database"]
async fn test_pool_transaction_rollback() {
    let config = get_test_config().expect("ORACLE_TEST_URL not set");

    let pool = PoolBuilder::new(config)
        .max_size(1)
        .build()
        .expect("Failed to build pool");

    // Create test table
    {
        let conn = pool.get().await.expect("Failed to get connection");
        conn.execute(
            "BEGIN EXECUTE IMMEDIATE 'CREATE TABLE pool_test (id NUMBER)'; EXCEPTION WHEN OTHERS THEN IF SQLCODE != -955 THEN RAISE; END IF; END;",
            &[]
        ).await.expect("Failed to create table");
        conn.execute("DELETE FROM pool_test", &[]).await.ok();
        conn.commit().await.expect("Failed to commit");
    }

    // Insert data without committing, then return connection to pool
    {
        let conn = pool.get().await.expect("Failed to get connection");
        conn.execute("INSERT INTO pool_test (id) VALUES (1)", &[])
            .await
            .expect("Insert failed");
        // Don't commit - let the pool recycle handle it
    }

    // Get connection again - recycle should have rolled back
    {
        let conn = pool.get().await.expect("Failed to get connection");
        let result = conn
            .query("SELECT COUNT(*) FROM pool_test", &[])
            .await
            .expect("Query failed");

        // Should be empty because recycle rolled back
        let row = &result.rows[0];
        match &row.values()[0] {
            oracle_rs::Value::String(s) => assert_eq!(s, "0"),
            oracle_rs::Value::Integer(i) => assert_eq!(*i, 0),
            other => panic!("Unexpected value: {:?}", other),
        }
    }

    // Cleanup
    {
        let conn = pool.get().await.expect("Failed to get connection");
        conn.execute("DROP TABLE pool_test", &[]).await.ok();
        conn.commit().await.ok();
    }
}

#[tokio::test]
#[ignore = "requires Oracle database"]
async fn test_pool_config_ext_trait() {
    let config = get_test_config().expect("ORACLE_TEST_URL not set");

    // Use the ConfigExt trait for simpler pool creation
    let pool = config.into_pool_with_size(5).expect("Failed to create pool");

    let conn = pool.get().await.expect("Failed to get connection");
    let result = conn
        .query("SELECT 1 FROM DUAL", &[])
        .await
        .expect("Query failed");

    assert_eq!(result.row_count(), 1);
}

#[tokio::test]
#[ignore = "requires Oracle database"]
async fn test_pool_timeout_configuration() {
    let config = get_test_config().expect("ORACLE_TEST_URL not set");

    // Create pool with custom timeouts
    let pool = PoolBuilder::new(config)
        .max_size(2)
        .wait_timeout(Some(Duration::from_secs(5)))
        .create_timeout(Some(Duration::from_secs(10)))
        .recycle_timeout(Some(Duration::from_secs(3)))
        .build()
        .expect("Failed to build pool");

    // Verify pool works with custom timeouts
    let conn = pool.get().await.expect("Failed to get connection");
    conn.query("SELECT 1 FROM DUAL", &[]).await.expect("Query failed");
}

#[test]
fn test_pool_builder_no_db() {
    // Test that pool builder works without database connection
    let config = Config::new("localhost", 1521, "FREEPDB1", "test", "test");

    let pool = PoolBuilder::new(config)
        .max_size(10)
        .wait_timeout(Some(Duration::from_secs(30)))
        .create_timeout(Some(Duration::from_secs(30)))
        .recycle_timeout(Some(Duration::from_secs(5)))
        .build();

    // Pool creation should succeed (connections are lazy)
    match &pool {
        Ok(_) => {}
        Err(e) => {
            println!("Pool build error: {:?}", e);
        }
    }
    assert!(pool.is_ok(), "Pool build failed");

    let pool = pool.unwrap();
    let status = pool.status();

    // No connections yet (lazy)
    assert_eq!(status.size, 0);
    assert_eq!(status.available, 0);
}
