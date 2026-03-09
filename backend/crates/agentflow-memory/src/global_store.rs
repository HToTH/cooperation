use anyhow::Result;
use chrono;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    Row, SqlitePool,
};
use std::str::FromStr;
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: String,
    pub workflow_id: String,
    pub agent_id: String,
    pub key: String,
    pub value: Value,
    pub created_at: String,
}

pub struct GlobalStore {
    pool: SqlitePool,
}

impl GlobalStore {
    pub async fn new(database_url: &str) -> Result<Self> {
        // Strip "sqlite:" prefix for SqliteConnectOptions if present
        let path = database_url
            .strip_prefix("sqlite://")
            .or_else(|| database_url.strip_prefix("sqlite:"))
            .unwrap_or(database_url);

        let opts =
            SqliteConnectOptions::from_str(&format!("sqlite:{}", path))?.create_if_missing(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(opts)
            .await?;

        let store = Self { pool };
        store.migrate().await?;
        Ok(store)
    }

    async fn migrate(&self) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS global_memory (
                id TEXT PRIMARY KEY,
                workflow_id TEXT NOT NULL,
                agent_id TEXT NOT NULL,
                key TEXT NOT NULL,
                value TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_workflow_id ON global_memory(workflow_id);
            CREATE INDEX IF NOT EXISTS idx_key ON global_memory(key);
            CREATE TABLE IF NOT EXISTS workflows (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                graph_json TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            "#,
        )
        .execute(&self.pool)
        .await?;

        info!("Global memory store migrated successfully");
        Ok(())
    }

    pub async fn write(&self, entry: MemoryEntry) -> Result<()> {
        sqlx::query(
            r#"
            INSERT OR REPLACE INTO global_memory (id, workflow_id, agent_id, key, value, created_at)
            VALUES (?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&entry.id)
        .bind(&entry.workflow_id)
        .bind(&entry.agent_id)
        .bind(&entry.key)
        .bind(serde_json::to_string(&entry.value)?)
        .bind(&entry.created_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn query_by_workflow(&self, workflow_id: &str) -> Result<Vec<MemoryEntry>> {
        let rows = sqlx::query(
            "SELECT id, workflow_id, agent_id, key, value, created_at FROM global_memory WHERE workflow_id = ? ORDER BY created_at ASC",
        )
        .bind(workflow_id)
        .fetch_all(&self.pool)
        .await?;

        let entries = rows
            .into_iter()
            .map(|row| {
                let value_str: String = row.get("value");
                Ok(MemoryEntry {
                    id: row.get("id"),
                    workflow_id: row.get("workflow_id"),
                    agent_id: row.get("agent_id"),
                    key: row.get("key"),
                    value: serde_json::from_str(&value_str)?,
                    created_at: row.get("created_at"),
                })
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(entries)
    }

    pub async fn query_by_key(&self, workflow_id: &str, key: &str) -> Result<Vec<MemoryEntry>> {
        let rows = sqlx::query(
            "SELECT id, workflow_id, agent_id, key, value, created_at FROM global_memory WHERE workflow_id = ? AND key = ? ORDER BY created_at ASC",
        )
        .bind(workflow_id)
        .bind(key)
        .fetch_all(&self.pool)
        .await?;

        let entries = rows
            .into_iter()
            .map(|row| {
                let value_str: String = row.get("value");
                Ok(MemoryEntry {
                    id: row.get("id"),
                    workflow_id: row.get("workflow_id"),
                    agent_id: row.get("agent_id"),
                    key: row.get("key"),
                    value: serde_json::from_str(&value_str)?,
                    created_at: row.get("created_at"),
                })
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(entries)
    }

    pub async fn save_workflow(&self, id: &str, name: &str, graph_json: &str) -> Result<()> {
        sqlx::query(
            "INSERT OR REPLACE INTO workflows (id, name, graph_json, updated_at) VALUES (?, ?, ?, ?)",
        )
        .bind(id)
        .bind(name)
        .bind(graph_json)
        .bind(chrono::Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn load_workflow(&self, id: &str) -> Result<Option<String>> {
        let row = sqlx::query("SELECT graph_json FROM workflows WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|r| r.get::<String, _>("graph_json")))
    }

    pub async fn delete_workflow(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM workflows WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn list_workflows(&self) -> Result<Vec<(String, String, String)>> {
        let rows =
            sqlx::query("SELECT id, name, updated_at FROM workflows ORDER BY updated_at DESC")
                .fetch_all(&self.pool)
                .await?;
        Ok(rows
            .into_iter()
            .map(|r| {
                (
                    r.get::<String, _>("id"),
                    r.get::<String, _>("name"),
                    r.get::<String, _>("updated_at"),
                )
            })
            .collect())
    }

    pub async fn search(&self, workflow_id: &str, query: &str) -> Result<Vec<Value>> {
        // Simple text search in JSON values - for production use a proper search engine
        let all = self.query_by_workflow(workflow_id).await?;
        let query_lower = query.to_lowercase();

        let results = all
            .into_iter()
            .filter(|entry| {
                entry
                    .value
                    .to_string()
                    .to_lowercase()
                    .contains(&query_lower)
                    || entry.key.to_lowercase().contains(&query_lower)
            })
            .map(|e| e.value)
            .collect();

        Ok(results)
    }
}
