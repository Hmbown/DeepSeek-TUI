//! Dump all memory entries from the production LanceDB database.
//!
//! Reads every table at ~/.deepseek/vector_memory/ and prints all records
//! with full content so we can inspect what the vector memory system stores.

use anyhow::{Context, Result};
use arrow_array::{Array, RecordBatch, StringArray, TimestampNanosecondArray};
use futures_util::TryStreamExt;
use lancedb::query::ExecutableQuery;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<()> {
    let home = dirs_next().context("failed to resolve home directory")?;
    let db_path = home.join(".deepseek").join("vector_memory");
    let conn_str = db_path.to_str().context("invalid path")?;

    println!("=== LanceDB Memory Dump ===");
    println!("Database: {}\n", db_path.display());

    // Connect to LanceDB
    let db = lancedb::connect(conn_str).execute().await?;

    let table_names = db.table_names().execute().await?;
    println!("Available tables: {:?}", table_names);
    println!();

    if table_names.is_empty() {
        println!("No tables found. Database directory listing:");
        let _ = std::process::Command::new("ls")
            .arg("-la")
            .arg(&db_path)
            .status();
        return Ok(());
    }

    for tname in &table_names {
        println!("{}", "#".repeat(80));
        println!("===== Table: {} =====", tname);

        let table = match db.open_table(tname).execute().await {
            Ok(t) => t,
            Err(e) => {
                eprintln!("  ERROR opening table: {e}");
                println!();
                continue;
            }
        };

        // Schema
        let schema = table.schema().await?;
        println!("  Schema:");
        for field in schema.fields() {
            println!("    {}: {:?}", field.name(), field.data_type());
        }
        println!();

        // Scan all rows
        let batches: Vec<RecordBatch> = table
            .query()
            .execute()
            .await?
            .try_collect()
            .await?;

        let total_rows: usize = batches.iter().map(|b| b.num_rows()).sum();
        println!("  Total records: {}", total_rows);
        println!();

        if total_rows == 0 {
            println!("  (empty table)");
            println!();
            continue;
        }

        // Print columns we care about
        let has_content = schema.fields().iter().any(|f| f.name() == "content");
        let has_summary = schema.fields().iter().any(|f| f.name() == "summary");

        for batch in &batches {
            let n = batch.num_rows();
            let content_col = if has_content {
                batch.column_by_name("content")
            } else if has_summary {
                batch.column_by_name("summary")
            } else {
                None
            };

            let source_col = batch.column_by_name("source");
            let tags_col = batch.column_by_name("tags");
            let session_col = batch.column_by_name("session_id");
            let created_col = batch.column_by_name("created_at");

            for row in 0..n {
                // Content
                let content = content_col
                    .and_then(|c| c.as_any().downcast_ref::<StringArray>())
                    .map(|a| a.value(row).to_string())
                    .unwrap_or_else(|| "(no content column)".to_string());

                // Source
                let source = source_col
                    .and_then(|c| c.as_any().downcast_ref::<StringArray>())
                    .map(|a| a.value(row).to_string())
                    .unwrap_or_else(|| "-".to_string());

                // Tags
                let tags = tags_col
                    .and_then(|c| c.as_any().downcast_ref::<StringArray>())
                    .map(|a| {
                        if a.is_null(row) { "-".to_string() }
                        else { a.value(row).to_string() }
                    })
                    .unwrap_or_else(|| "-".to_string());

                // Session
                let session = session_col
                    .and_then(|c| c.as_any().downcast_ref::<StringArray>())
                    .map(|a| {
                        if a.is_null(row) { "-".to_string() }
                        else { a.value(row).to_string() }
                    })
                    .unwrap_or_else(|| "-".to_string());

                // Timestamp
                let ts = created_col
                    .and_then(|c| c.as_any().downcast_ref::<TimestampNanosecondArray>())
                    .and_then(|a| {
                        if a.is_null(row) { return None; }
                        let nanos = a.value(row) as i64;
                        let secs = nanos / 1_000_000_000;
                        chrono::DateTime::from_timestamp(secs, 0)
                            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                    })
                    .unwrap_or_else(|| "unknown".to_string());

                println!("  --- Record ---");
                println!("  source:     {}", source);
                println!("  tags:       {}", tags);
                println!("  session:    {}", session);
                println!("  created_at: {}", ts);
                println!("  content[{}B]:", content.len());
                // Print content with indentation
                for line in content.lines() {
                    println!("    {}", line);
                }
                println!();
            }
        }
    }

    println!("{}", "#".repeat(80));
    println!("\n=== End of dump ===");
    Ok(())
}

fn dirs_next() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| {
            if cfg!(windows) {
                std::env::var_os("USERPROFILE")
            } else {
                None
            }
        })
        .map(PathBuf::from)
}
