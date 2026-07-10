use std::{collections::HashMap, path::Path};

use anyhow::Result;
use chrono::{Duration, Local};
use rusqlite::{params, types::Type, Connection, OptionalExtension, Row, ToSql};
use serde::Serialize;

use crate::model::{OcrError, Page, RecognitionResult, ServiceId};

const TASK_COLUMNS: &str = "id, service, status, input_path, options_json, \
                            progress_page, total_pages, error_kind, error_msg, created_at";

pub struct Store(rusqlite::Connection);

pub struct NewTask {
    pub id: String,
    pub service: ServiceId,
    pub input_path: String,
    pub options_json: String,
}

pub(crate) struct AdmittedTask {
    pub task: NewTask,
    pub persist_result: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskRow {
    pub id: String,
    pub service: ServiceId,
    pub status: String,
    pub input_path: String,
    pub options_json: String,
    pub progress_page: u32,
    pub total_pages: u32,
    pub error_kind: Option<String>,
    pub error_msg: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct HistoryRow {
    pub task_id: String,
    pub file_name: String,
    pub snippet: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct UsageRow {
    pub date: String,
    pub service: ServiceId,
    pub pages: u32,
}

impl Store {
    pub fn open(path: &Path) -> Result<Store> {
        let connection = Connection::open(path)?;
        connection.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE IF NOT EXISTS tasks(
               id TEXT PRIMARY KEY, created_at INTEGER NOT NULL,
               service TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'pending',
               input_path TEXT NOT NULL, options_json TEXT NOT NULL DEFAULT '{}',
               progress_page INTEGER NOT NULL DEFAULT 0, total_pages INTEGER NOT NULL DEFAULT 0,
               error_kind TEXT, error_msg TEXT,
               persist_result INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE IF NOT EXISTS results(
               task_id TEXT PRIMARY KEY REFERENCES tasks(id) ON DELETE CASCADE,
               markdown TEXT NOT NULL, blocks_json TEXT NOT NULL, page_count INTEGER NOT NULL);
             CREATE VIRTUAL TABLE IF NOT EXISTS history_fts
               USING fts5(task_id UNINDEXED, file_name, markdown);
             CREATE TABLE IF NOT EXISTS usage(
               date TEXT NOT NULL, service TEXT NOT NULL, pages INTEGER NOT NULL DEFAULT 0,
               PRIMARY KEY(date, service));
             CREATE TABLE IF NOT EXISTS settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);",
        )?;
        ensure_persist_result_column(&connection)?;
        Ok(Store(connection))
    }

    pub fn insert_task(&self, task: &NewTask, persist_result: bool) -> Result<()> {
        self.0.execute(
            "INSERT INTO tasks(
               id, created_at, service, input_path, options_json, persist_result
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                task.id,
                Local::now().timestamp(),
                service_name(task.service),
                task.input_path,
                task.options_json,
                persist_result
            ],
        )?;
        Ok(())
    }

    pub fn update_status(
        &self,
        id: &str,
        status: &str,
        progress: Option<(u32, u32)>,
        error: Option<&OcrError>,
    ) -> Result<()> {
        let (progress_page, total_pages) = progress
            .map(|(page, total)| (Some(i64::from(page)), Some(i64::from(total))))
            .unwrap_or((None, None));
        let (error_kind, error_msg) = error
            .map(error_fields)
            .map(|(kind, message)| (Some(kind), Some(message)))
            .unwrap_or((None, None));
        self.0.execute(
            "UPDATE tasks SET status = ?1,
             progress_page = COALESCE(?2, progress_page),
             total_pages = COALESCE(?3, total_pages),
             error_kind = ?4, error_msg = ?5 WHERE id = ?6",
            params![
                status,
                progress_page,
                total_pages,
                error_kind,
                error_msg,
                id
            ],
        )?;
        Ok(())
    }

    pub fn update_status_if_active(
        &self,
        id: &str,
        status: &str,
        progress: Option<(u32, u32)>,
        error: Option<&OcrError>,
    ) -> Result<bool> {
        let (progress_page, total_pages) = progress
            .map(|(page, total)| (Some(i64::from(page)), Some(i64::from(total))))
            .unwrap_or((None, None));
        let (error_kind, error_msg) = error
            .map(error_fields)
            .map(|(kind, message)| (Some(kind), Some(message)))
            .unwrap_or((None, None));
        let changed = self.0.execute(
            "UPDATE tasks SET status = ?1,
             progress_page = COALESCE(?2, progress_page),
             total_pages = COALESCE(?3, total_pages),
             error_kind = ?4, error_msg = ?5
             WHERE id = ?6 AND status NOT IN ('done','canceled')",
            params![
                status,
                progress_page,
                total_pages,
                error_kind,
                error_msg,
                id
            ],
        )?;
        Ok(changed == 1)
    }

    pub fn cancel_task(&self, id: &str) -> Result<bool> {
        let changed = self.0.execute(
            "UPDATE tasks SET status = 'canceled', error_kind = NULL, error_msg = NULL
             WHERE id = ?1 AND status NOT IN ('done','canceled')",
            [id],
        )?;
        Ok(changed == 1)
    }

    pub(crate) fn retry_task(&self, id: &str) -> Result<Option<AdmittedTask>> {
        let transaction = self.0.unchecked_transaction()?;
        let task = transaction
            .query_row(
                "SELECT id, service, input_path, options_json, persist_result FROM tasks
                 WHERE id = ?1 AND status = 'failed'",
                [id],
                |row| {
                    Ok(AdmittedTask {
                        task: NewTask {
                            id: row.get(0)?,
                            service: service_from_str(1, row.get(1)?)?,
                            input_path: row.get(2)?,
                            options_json: row.get(3)?,
                        },
                        persist_result: row.get(4)?,
                    })
                },
            )
            .optional()?;
        let Some(task) = task else {
            transaction.rollback()?;
            return Ok(None);
        };
        transaction.execute(
            "UPDATE tasks SET status = 'pending', progress_page = 0, total_pages = 0,
             error_kind = NULL, error_msg = NULL WHERE id = ?1",
            [id],
        )?;
        transaction.commit()?;
        Ok(Some(task))
    }

    pub fn list_tasks(&self, status_filter: Option<&str>) -> Result<Vec<TaskRow>> {
        let sql = format!("SELECT {TASK_COLUMNS} FROM tasks");
        match status_filter {
            Some(status) => self.query_tasks(&(sql + " WHERE status = ?1"), &[&status]),
            None => self.query_tasks(&sql, &[]),
        }
    }

    pub(crate) fn unfinished_tasks(&self) -> Result<Vec<AdmittedTask>> {
        let mut statement = self.0.prepare(
            "SELECT id, service, input_path, options_json, persist_result FROM tasks
             WHERE status NOT IN ('done','canceled')",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(AdmittedTask {
                task: NewTask {
                    id: row.get(0)?,
                    service: service_from_str(1, row.get(1)?)?,
                    input_path: row.get(2)?,
                    options_json: row.get(3)?,
                },
                persist_result: row.get(4)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn save_result(
        &self,
        task_id: &str,
        file_name: &str,
        result: &RecognitionResult,
    ) -> Result<()> {
        let transaction = self.0.unchecked_transaction()?;
        write_result(&transaction, task_id, file_name, result)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn complete_task(
        &self,
        task_id: &str,
        file_name: &str,
        result: &RecognitionResult,
        date: &str,
        service: ServiceId,
        persist_result: bool,
    ) -> Result<bool> {
        let transaction = self.0.unchecked_transaction()?;
        let changed = transaction.execute(
            "UPDATE tasks SET status = 'done', error_kind = NULL, error_msg = NULL
             WHERE id = ?1 AND status NOT IN ('done','canceled')",
            [task_id],
        )?;
        if changed == 0 {
            transaction.rollback()?;
            return Ok(false);
        }
        if persist_result {
            write_result(&transaction, task_id, file_name, result)?;
        }
        write_usage(&transaction, date, service, result.page_count)?;
        transaction.commit()?;
        Ok(true)
    }

    pub fn get_result(&self, task_id: &str) -> Result<Option<RecognitionResult>> {
        let stored: Option<(String, String, u32)> = self
            .0
            .query_row(
                "SELECT markdown, blocks_json, page_count FROM results WHERE task_id = ?1",
                [task_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        stored
            .map(|(markdown, pages, page_count)| {
                Ok(RecognitionResult {
                    markdown,
                    page_count,
                    pages: serde_json::from_str::<Vec<Page>>(&pages)?,
                })
            })
            .transpose()
    }

    pub fn search_history(&self, query: &str) -> Result<Vec<HistoryRow>> {
        let mut statement = self.0.prepare(
            "SELECT history_fts.task_id, history_fts.file_name,
                    snippet(history_fts, 2, '', '', '…', 12), tasks.created_at
             FROM history_fts JOIN tasks ON tasks.id = history_fts.task_id
             WHERE history_fts MATCH ?1 ORDER BY tasks.created_at DESC",
        )?;
        let query = format!("{query}*");
        let rows = statement.query_map([query], |row| {
            Ok(HistoryRow {
                task_id: row.get(0)?,
                file_name: row.get(1)?,
                snippet: row.get(2)?,
                created_at: row.get(3)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn add_usage(&self, date: &str, service: ServiceId, pages: u32) -> Result<()> {
        write_usage(&self.0, date, service, pages)
    }

    pub fn usage_since(&self, days: u32) -> Result<Vec<UsageRow>> {
        let offset = i64::from(days.saturating_sub(1));
        let start = (Local::now().date_naive() - Duration::days(offset)).to_string();
        let mut statement = self.0.prepare(
            "SELECT date, service, pages FROM usage WHERE date >= ?1 ORDER BY date DESC, service",
        )?;
        let rows = statement.query_map([start], usage_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn get_settings(&self) -> Result<HashMap<String, String>> {
        let mut statement = self.0.prepare("SELECT key, value FROM settings")?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<HashMap<_, _>>>()?)
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .0
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()?)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.0.execute(
            "INSERT INTO settings(key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn set_settings(&self, settings: &HashMap<String, String>) -> Result<()> {
        let transaction = self.0.unchecked_transaction()?;
        for (key, value) in settings {
            transaction.execute(
                "INSERT INTO settings(key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    fn query_tasks(&self, sql: &str, params: &[&dyn ToSql]) -> Result<Vec<TaskRow>> {
        let mut statement = self.0.prepare(sql)?;
        let rows = statement.query_map(params, task_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

fn ensure_persist_result_column(connection: &Connection) -> Result<()> {
    let mut statement = connection.prepare("PRAGMA table_info(tasks)")?;
    let columns = statement.query_map([], |row| row.get::<_, String>(1))?;
    let mut found = false;
    for column in columns {
        found |= column? == "persist_result";
    }
    if !found {
        connection.execute(
            "ALTER TABLE tasks ADD COLUMN persist_result INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    Ok(())
}

fn write_result(
    connection: &Connection,
    task_id: &str,
    file_name: &str,
    result: &RecognitionResult,
) -> Result<()> {
    let pages_json = serde_json::to_string(&result.pages)?;
    connection.execute(
        "INSERT INTO results(task_id, markdown, blocks_json, page_count)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(task_id) DO UPDATE SET markdown = excluded.markdown,
         blocks_json = excluded.blocks_json, page_count = excluded.page_count",
        params![task_id, result.markdown, pages_json, result.page_count],
    )?;
    connection.execute("DELETE FROM history_fts WHERE task_id = ?1", [task_id])?;
    connection.execute(
        "INSERT INTO history_fts(task_id, file_name, markdown) VALUES (?1, ?2, ?3)",
        params![task_id, file_name, result.markdown],
    )?;
    Ok(())
}

fn write_usage(connection: &Connection, date: &str, service: ServiceId, pages: u32) -> Result<()> {
    connection.execute(
        "INSERT INTO usage(date, service, pages) VALUES (?1, ?2, ?3)
         ON CONFLICT(date, service) DO UPDATE SET pages = pages + excluded.pages",
        params![date, service_name(service), pages],
    )?;
    Ok(())
}

fn service_name(service: ServiceId) -> &'static str {
    match service {
        ServiceId::Vl16 => "vl16",
        ServiceId::PpOcrV6 => "pp_ocr_v6",
        ServiceId::StructureV3 => "structure_v3",
    }
}

fn service_from_str(index: usize, value: String) -> rusqlite::Result<ServiceId> {
    match value.as_str() {
        "vl16" => Ok(ServiceId::Vl16),
        "pp_ocr_v6" => Ok(ServiceId::PpOcrV6),
        "structure_v3" => Ok(ServiceId::StructureV3),
        _ => Err(rusqlite::Error::FromSqlConversionFailure(
            index,
            Type::Text,
            format!("unknown service: {value}").into(),
        )),
    }
}

fn task_row(row: &Row<'_>) -> rusqlite::Result<TaskRow> {
    Ok(TaskRow {
        id: row.get(0)?,
        service: service_from_str(1, row.get(1)?)?,
        status: row.get(2)?,
        input_path: row.get(3)?,
        options_json: row.get(4)?,
        progress_page: row.get(5)?,
        total_pages: row.get(6)?,
        error_kind: row.get(7)?,
        error_msg: row.get(8)?,
        created_at: row.get(9)?,
    })
}

fn usage_row(row: &Row<'_>) -> rusqlite::Result<UsageRow> {
    Ok(UsageRow {
        date: row.get(0)?,
        service: service_from_str(1, row.get(1)?)?,
        pages: row.get(2)?,
    })
}

fn error_fields(error: &OcrError) -> (&'static str, String) {
    let kind = match error {
        OcrError::Auth => "auth",
        OcrError::Quota => "quota",
        OcrError::Network(_) => "network",
        OcrError::Server(_) => "server",
        OcrError::Parse(_) => "parse",
    };
    (kind, error.to_string())
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
        time::Duration as StdDuration,
    };

    use tokio::{sync::mpsc, time::timeout};

    use super::*;
    use crate::{
        api::{mock::MockOcr, OcrService},
        model::{OcrError, RecognitionResult, ServiceId},
        queue::{Queue, QueueEvent},
    };

    fn tmp_store() -> (tempfile::TempDir, Store) {
        let d = tempfile::tempdir().unwrap();
        let s = Store::open(&d.path().join("t.db")).unwrap();
        (d, s)
    }

    #[test]
    fn task_lifecycle_and_resume() {
        let (_d, s) = tmp_store();
        s.insert_task(
            &NewTask {
                id: "t1".into(),
                service: ServiceId::Vl16,
                input_path: "a.pdf".into(),
                options_json: "{}".into(),
            },
            true,
        )
        .unwrap();
        s.update_status("t1", "processing", Some((3, 10)), None)
            .unwrap();
        assert_eq!(s.unfinished_tasks().unwrap().len(), 1);
        s.update_status("t1", "done", None, None).unwrap();
        assert!(s.unfinished_tasks().unwrap().is_empty());
    }

    #[test]
    fn fts_search_finds_content() {
        let (_d, s) = tmp_store();
        s.insert_task(
            &NewTask {
                id: "t1".into(),
                service: ServiceId::Vl16,
                input_path: "讲义.pdf".into(),
                options_json: "{}".into(),
            },
            true,
        )
        .unwrap();
        let r = RecognitionResult {
            markdown: "卷积神经网络基础".into(),
            page_count: 1,
            pages: vec![],
        };
        s.save_result("t1", "讲义.pdf", &r).unwrap();
        assert_eq!(s.get_result("t1").unwrap().unwrap(), r);
        assert_eq!(s.search_history("卷积").unwrap().len(), 1);
        assert!(s.search_history("不存在词").unwrap().is_empty());
    }

    #[test]
    fn usage_accumulates_and_settings_roundtrip() {
        let (_d, s) = tmp_store();
        let today = chrono::Local::now().date_naive().to_string();
        s.add_usage(&today, ServiceId::Vl16, 5).unwrap();
        s.add_usage(&today, ServiceId::Vl16, 3).unwrap();
        assert_eq!(s.usage_since(1).unwrap()[0].pages, 8);
        s.set_setting("proxy_mode", "direct").unwrap();
        assert_eq!(s.get_setting("proxy_mode").unwrap().unwrap(), "direct");
        s.set_setting("theme", "dark").unwrap();
        assert_eq!(
            s.get_settings().unwrap(),
            std::collections::HashMap::from([
                ("proxy_mode".to_string(), "direct".to_string()),
                ("theme".to_string(), "dark".to_string()),
            ])
        );
    }

    #[test]
    fn batch_settings_roll_back_every_key_on_failure() {
        let (_d, s) = tmp_store();
        s.0.execute_batch(
            "CREATE TRIGGER fail_privacy_setting BEFORE INSERT ON settings
             WHEN NEW.key = 'privacy_mode'
             BEGIN SELECT RAISE(ABORT, 'forced settings failure'); END;",
        )
        .unwrap();
        let settings = HashMap::from([
            ("theme".to_string(), "dark".to_string()),
            ("privacy_mode".to_string(), "1".to_string()),
        ]);

        assert!(s.set_settings(&settings).is_err());
        assert!(s.get_settings().unwrap().is_empty());
    }

    #[test]
    fn opening_legacy_database_adds_admission_policy_column() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("legacy.db");
        let legacy = Connection::open(&path).unwrap();
        legacy
            .execute_batch(
                "CREATE TABLE tasks(
                   id TEXT PRIMARY KEY, created_at INTEGER NOT NULL,
                   service TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'pending',
                   input_path TEXT NOT NULL, options_json TEXT NOT NULL DEFAULT '{}',
                   progress_page INTEGER NOT NULL DEFAULT 0,
                   total_pages INTEGER NOT NULL DEFAULT 0,
                   error_kind TEXT, error_msg TEXT);
                 INSERT INTO tasks(id, created_at, service, input_path)
                 VALUES ('legacy', 1, 'vl16', 'legacy.png');",
            )
            .unwrap();
        drop(legacy);

        let store = Store::open(&path).unwrap();
        let tasks = store.unfinished_tasks().unwrap();
        assert_eq!(tasks.len(), 1);
        assert!(!tasks[0].persist_result);
    }

    #[test]
    fn rows_serialize_for_ipc() {
        let task = TaskRow {
            id: "t1".into(),
            service: ServiceId::Vl16,
            status: "done".into(),
            input_path: "a.png".into(),
            options_json: "{}".into(),
            progress_page: 1,
            total_pages: 1,
            error_kind: None,
            error_msg: None,
            created_at: 1,
        };
        let history = HistoryRow {
            task_id: "t1".into(),
            file_name: "a.png".into(),
            snippet: "text".into(),
            created_at: 1,
        };
        let usage = UsageRow {
            date: "2026-07-10".into(),
            service: ServiceId::Vl16,
            pages: 1,
        };

        assert_eq!(serde_json::to_value(task).unwrap()["status"], "done");
        assert_eq!(serde_json::to_value(history).unwrap()["file_name"], "a.png");
        assert_eq!(serde_json::to_value(usage).unwrap()["pages"], 1);
    }

    #[tokio::test]
    async fn queue_storage_failure_emits_failed_without_partial_completion() {
        let (_d, s) = tmp_store();
        s.0.execute_batch(
            "CREATE TRIGGER fail_usage BEFORE INSERT ON usage
             BEGIN SELECT RAISE(ABORT, 'forced usage failure'); END;",
        )
        .unwrap();
        let store = Arc::new(Mutex::new(s));
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut services: HashMap<ServiceId, Arc<dyn OcrService>> = HashMap::new();
        services.insert(ServiceId::Vl16, Arc::new(MockOcr::new()));
        let queue = Queue::new(
            store.clone(),
            services,
            1,
            tx,
            StdDuration::from_millis(1),
            true,
        );
        queue.submit(NewTask {
            id: "broken".into(),
            service: ServiceId::Vl16,
            input_path: "a.png".into(),
            options_json: "{}".into(),
        });
        let error = timeout(StdDuration::from_secs(1), async {
            loop {
                if let QueueEvent::Failed { error, .. } =
                    rx.recv().await.expect("queue event channel closed")
                {
                    return error;
                }
            }
        })
        .await
        .expect("timed out waiting for storage failure");
        assert!(matches!(error, OcrError::Parse(message) if message.contains("forced usage")));
        let store = store.lock().unwrap();
        assert_eq!(store.list_tasks(Some("failed")).unwrap().len(), 1);
        assert!(store.get_result("broken").unwrap().is_none());
        assert!(store.usage_since(1).unwrap().is_empty());
    }

    #[test]
    fn complete_task_rolls_back_status_result_and_usage_together() {
        let (_d, s) = tmp_store();
        s.insert_task(
            &NewTask {
                id: "atomic".into(),
                service: ServiceId::Vl16,
                input_path: "a.png".into(),
                options_json: "{}".into(),
            },
            true,
        )
        .unwrap();
        s.0.execute_batch(
            "CREATE TRIGGER fail_usage BEFORE INSERT ON usage
             BEGIN SELECT RAISE(ABORT, 'forced usage failure'); END;",
        )
        .unwrap();
        let result = RecognitionResult {
            markdown: "atomic result".into(),
            page_count: 1,
            pages: vec![],
        };
        assert!(s
            .complete_task(
                "atomic",
                "a.png",
                &result,
                &Local::now().date_naive().to_string(),
                ServiceId::Vl16,
                true,
            )
            .is_err());
        assert_eq!(s.list_tasks(None).unwrap()[0].status, "pending");
        assert!(s.get_result("atomic").unwrap().is_none());
        assert!(s.usage_since(1).unwrap().is_empty());
    }
}
