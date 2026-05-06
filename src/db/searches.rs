use anyhow::anyhow;
use chrono::Utc;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::types::{CarsSearchParams, ClassifiedsSearchParams};

#[derive(Debug, Serialize, Deserialize)]
pub struct SavedSearchRow {
    pub id: i64,
    pub name: String,
    pub parameters: String,
    pub platform: String,
    pub last_run_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "platform")]
pub enum SavedSearchParams {
    #[serde(rename = "classifieds")]
    Classifieds(ClassifiedsSearchParams),
    #[serde(rename = "cars")]
    Cars(CarsSearchParams),
}

pub fn save_search(
    conn: &Connection,
    name: &str,
    search_params: &SavedSearchParams,
) -> anyhow::Result<SavedSearchRow> {
    let now = Utc::now().to_rfc3339();
    let platform = match search_params {
        SavedSearchParams::Classifieds(_) => "classifieds",
        SavedSearchParams::Cars(_) => "cars",
    };
    let json = serde_json::to_string(search_params)?;
    conn.execute(
        "INSERT INTO saved_searches (name, parameters, platform, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![name, json, platform, &now],
    )?;
    let id = conn.last_insert_rowid();
    get_by_id(conn, id)?.ok_or_else(|| anyhow!("insert failed"))
}

pub fn list_saved_searches(conn: &Connection) -> anyhow::Result<Vec<SavedSearchRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, parameters, platform, last_run_at, created_at FROM saved_searches ORDER BY created_at DESC",
    )?;
    let rows = stmt
        .query_map([], map_row)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn delete_saved_search(conn: &Connection, id: i64) -> anyhow::Result<bool> {
    let deleted = conn.execute("DELETE FROM saved_searches WHERE id = ?1", params![id])?;
    Ok(deleted > 0)
}

pub fn get_by_id(conn: &Connection, id: i64) -> anyhow::Result<Option<SavedSearchRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, parameters, platform, last_run_at, created_at FROM saved_searches WHERE id = ?1",
    )?;
    let mut rows = stmt.query_map(params![id], map_row)?;
    Ok(rows.next().transpose()?)
}

pub fn update_last_run(conn: &Connection, id: i64) -> anyhow::Result<()> {
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE saved_searches SET last_run_at = ?1 WHERE id = ?2",
        params![&now, id],
    )?;
    Ok(())
}

pub fn parse_params(row: &SavedSearchRow) -> anyhow::Result<SavedSearchParams> {
    Ok(serde_json::from_str(&row.parameters)?)
}

fn map_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<SavedSearchRow> {
    Ok(SavedSearchRow {
        id: r.get(0)?,
        name: r.get(1)?,
        parameters: r.get(2)?,
        platform: r.get(3)?,
        last_run_at: r.get(4)?,
        created_at: r.get(5)?,
    })
}
