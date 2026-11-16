use crate::config::AppConfig;
use couch_rs::Client;
use serde_json::{json, Value};
use sqlx::mysql::MySqlPoolOptions;
use sqlx::{Column, Row};
use std::error::Error;

pub async fn migrate_table(config: &AppConfig, table: &str, query: &str) -> Result<(), Box<dyn Error>> {
    let mysql_url = format!(
        "mysql://{}:{}@{}/{}",
        config.mysql_user, config.mysql_password, config.mysql_host, config.mysql_database
    );

    println!("Connecting to MySQL: {}/{}", config.mysql_host, config.mysql_database);
    let pool = MySqlPoolOptions::new()
        .max_connections(5)
        .connect(&mysql_url)
        .await?;

    println!("Running query on table: {}", table);
    let rows = sqlx::query(sqlx::AssertSqlSafe(query))
        .fetch_all(&pool)
        .await?;

    println!("Found {} rows", rows.len());

    let couch_url = &config.host;
    let couch_user = &config.user;
    let couch_pass = &config.password;
    let client = Client::new(couch_url, couch_user, couch_pass)?;

    let db_name = if config.database.is_empty() { table } else { &config.database };
    println!("Ensuring CouchDB database: {}", db_name);

    let db = client.db(db_name).await?;

    let columns: Vec<String> = if let Some(first) = rows.first() {
        first.columns().iter().map(|c| c.name().to_string()).collect()
    } else {
        println!("No rows to migrate");
        return Ok(());
    };

    println!("Columns: {:?}", columns);

    let mut docs: Vec<Value> = Vec::new();
    for row in &rows {
        let mut doc = json!({});
        for col in &columns {
            let val: Value = match row.try_get::<String, _>(col.as_str()) {
                Ok(v) => json!(v),
                Err(_) => match row.try_get::<i64, _>(col.as_str()) {
                    Ok(v) => json!(v),
                    Err(_) => match row.try_get::<f64, _>(col.as_str()) {
                        Ok(v) => json!(v),
                        Err(_) => match row.try_get::<bool, _>(col.as_str()) {
                            Ok(v) => json!(v),
                            Err(_) => Value::Null,
                        },
                    },
                },
            };
            doc[col] = val;
        }
        docs.push(doc);
    }

    println!("Inserting {} documents into CouchDB...", docs.len());
    let mut success = 0;
    let mut failed = 0;

    for doc in &docs {
        let mut doc = doc.clone();
        match db.create(&mut doc).await {
            Ok(_) => success += 1,
            Err(e) => {
                failed += 1;
                eprintln!("Failed to insert doc: {}", e);
            }
        }
    }

    println!("Done. {} inserted, {} failed", success, failed);
    pool.close().await;

    Ok(())
}

pub async fn migrate_query(config: &AppConfig, query: &str, target_db: &str) -> Result<(), Box<dyn Error>> {
    let mysql_url = format!(
        "mysql://{}:{}@{}/{}",
        config.mysql_user, config.mysql_password, config.mysql_host, config.mysql_database
    );

    println!("Connecting to MySQL: {}/{}", config.mysql_host, config.mysql_database);
    let pool = MySqlPoolOptions::new()
        .max_connections(5)
        .connect(&mysql_url)
        .await?;

    println!("Running custom query");
    let rows = sqlx::query(sqlx::AssertSqlSafe(query))
        .fetch_all(&pool)
        .await?;

    println!("Found {} rows", rows.len());

    let couch_url = &config.host;
    let couch_user = &config.user;
    let couch_pass = &config.password;
    let client = Client::new(couch_url, couch_user, couch_pass)?;

    let db = client.db(target_db).await?;

    let columns: Vec<String> = if let Some(first) = rows.first() {
        first.columns().iter().map(|c| c.name().to_string()).collect()
    } else {
        println!("No rows to migrate");
        return Ok(());
    };

    println!("Columns: {:?}", columns);

    let mut docs: Vec<Value> = Vec::new();
    for row in &rows {
        let mut doc = json!({});
        for col in &columns {
            let val: Value = match row.try_get::<String, _>(col.as_str()) {
                Ok(v) => json!(v),
                Err(_) => match row.try_get::<i64, _>(col.as_str()) {
                    Ok(v) => json!(v),
                    Err(_) => match row.try_get::<f64, _>(col.as_str()) {
                        Ok(v) => json!(v),
                        Err(_) => match row.try_get::<bool, _>(col.as_str()) {
                            Ok(v) => json!(v),
                            Err(_) => Value::Null,
                        },
                    },
                },
            };
            doc[col] = val;
        }
        docs.push(doc);
    }

    println!("Inserting {} documents into CouchDB...", docs.len());
    let mut success = 0;
    let mut failed = 0;

    for doc in &docs {
        let mut doc = doc.clone();
        match db.create(&mut doc).await {
            Ok(_) => success += 1,
            Err(e) => {
                failed += 1;
                eprintln!("Failed to insert doc: {}", e);
            }
        }
    }

    println!("Done. {} inserted, {} failed", success, failed);
    pool.close().await;

    Ok(())
}
