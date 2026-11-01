use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::Path;

const COLS: &str = "id,path,name,language,status,description,tags,notes,deps,commits,readme,git_remote,structure,last_commit_at,analyzed_at,ai_source,updated_at,project_type";

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS projects (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    path TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    language TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'pending',
    description TEXT,
    tags TEXT NOT NULL DEFAULT '[]',
    notes TEXT,
    deps TEXT NOT NULL DEFAULT '[]',
    commits TEXT NOT NULL DEFAULT '[]',
    readme TEXT,
    git_remote TEXT,
    structure TEXT NOT NULL DEFAULT '[]',
    last_commit_at INTEGER,
    analyzed_at INTEGER,
    ai_source TEXT,
    updated_at INTEGER NOT NULL DEFAULT 0,
    project_type TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS idx_projects_status ON projects(status);
";

pub struct Db {
    conn: Connection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRow {
    pub id: i64,
    pub path: String,
    pub name: String,
    pub language: String,
    pub status: String,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub notes: Option<String>,
    pub deps: Vec<String>,
    pub commits: Vec<String>,
    pub readme: Option<String>,
    pub git_remote: Option<String>,
    pub structure: Vec<String>,
    pub last_commit_at: Option<i64>,
    pub analyzed_at: Option<i64>,
    pub ai_source: Option<String>,
    pub updated_at: i64,
    /// AI 判定的项目类型（固定枚举，见 analyzer::PROJECT_TYPES）；空串表示未分类。
    pub project_type: String,
}

/// 扫描/手动添加时的新项目输入。
#[derive(Debug, Clone)]
pub struct NewProject {
    pub path: String,
    pub name: String,
    pub language: String,
    pub status: String,
    pub deps: Vec<String>,
    pub commits: Vec<String>,
    pub readme: Option<String>,
    pub git_remote: Option<String>,
    pub structure: Vec<String>,
    pub last_commit_at: Option<i64>,
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn to_json(v: &[String]) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "[]".into())
}

fn json_vec(s: &str) -> Vec<String> {
    serde_json::from_str(s).unwrap_or_default()
}

/// 老库升级：补齐后加的列（SQLite 的 CREATE TABLE IF NOT EXISTS 不会改已存在的表）。
/// 注意：依赖新列的索引必须放在这里建，不能写进 SCHEMA——
/// 老库执行 SCHEMA 时列还不存在，会直接报 "no such column" 导致应用打不开库。
fn migrate(conn: &Connection) -> Result<(), String> {
    let cols: Vec<String> = conn
        .prepare("PRAGMA table_info(projects)")
        .map_err(|e| e.to_string())?
        .query_map([], |r| r.get::<_, String>("name"))
        .map_err(|e| e.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    if !cols.iter().any(|c| c == "project_type") {
        conn.execute_batch("ALTER TABLE projects ADD COLUMN project_type TEXT NOT NULL DEFAULT ''")
            .map_err(|e| e.to_string())?;
    }
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_projects_type ON projects(project_type);
         CREATE INDEX IF NOT EXISTS idx_projects_language ON projects(language);",
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn row_to_project(row: &rusqlite::Row) -> rusqlite::Result<ProjectRow> {
    Ok(ProjectRow {
        id: row.get("id")?,
        path: row.get("path")?,
        name: row.get("name")?,
        language: row.get("language")?,
        status: row.get("status")?,
        description: row.get("description")?,
        tags: json_vec(&row.get::<_, String>("tags")?),
        notes: row.get("notes")?,
        deps: json_vec(&row.get::<_, String>("deps")?),
        commits: json_vec(&row.get::<_, String>("commits")?),
        readme: row.get("readme")?,
        git_remote: row.get("git_remote")?,
        structure: json_vec(&row.get::<_, String>("structure")?),
        last_commit_at: row.get("last_commit_at")?,
        analyzed_at: row.get("analyzed_at")?,
        ai_source: row.get("ai_source")?,
        updated_at: row.get("updated_at")?,
        project_type: row.get::<_, Option<String>>("project_type")?.unwrap_or_default(),
    })
}

impl Db {
    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
        let conn = Connection::open(path).map_err(|e| e.to_string())?;
        conn.execute_batch(SCHEMA).map_err(|e| e.to_string())?;
        migrate(&conn)?;
        Ok(Db { conn })
    }

    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self, String> {
        let conn = Connection::open_in_memory().map_err(|e| e.to_string())?;
        conn.execute_batch(SCHEMA).map_err(|e| e.to_string())?;
        migrate(&conn)?;
        Ok(Db { conn })
    }

    /// 按路径 upsert：信号字段更新，AI 描述/标签/备注/状态保留。
    /// 状态例外：旧记录是 candidate 而新识别结果不是 → 升级为识别结果。
    pub fn upsert(&self, p: &NewProject) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT INTO projects(path,name,language,status,deps,commits,readme,git_remote,structure,last_commit_at,updated_at)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)
                 ON CONFLICT(path) DO UPDATE SET
                    name=excluded.name,
                    language=excluded.language,
                    deps=excluded.deps,
                    commits=excluded.commits,
                    readme=excluded.readme,
                    git_remote=excluded.git_remote,
                    structure=excluded.structure,
                    last_commit_at=excluded.last_commit_at,
                    updated_at=excluded.updated_at,
                    status=CASE WHEN projects.status='candidate' AND excluded.status!='candidate'
                                THEN excluded.status ELSE projects.status END",
                params![
                    p.path,
                    p.name,
                    p.language,
                    p.status,
                    to_json(&p.deps),
                    to_json(&p.commits),
                    p.readme,
                    p.git_remote,
                    to_json(&p.structure),
                    p.last_commit_at,
                    now()
                ],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn get_id_by_path(&self, path: &str) -> Result<Option<i64>, String> {
        self.conn
            .query_row(
                "SELECT id FROM projects WHERE path = ?1",
                params![path],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())
    }

    pub fn list(&self) -> Result<Vec<ProjectRow>, String> {
        let sql = format!(
            "SELECT {COLS} FROM projects ORDER BY COALESCE(last_commit_at, updated_at) DESC"
        );
        let mut stmt = self.conn.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], row_to_project)
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        Ok(rows)
    }

    pub fn get(&self, id: i64) -> Result<Option<ProjectRow>, String> {
        let sql = format!("SELECT {COLS} FROM projects WHERE id = ?1");
        self.conn
            .query_row(&sql, params![id], row_to_project)
            .optional()
            .map_err(|e| e.to_string())
    }

    /// 写入 AI 分析结果（含项目类型），状态置为 ok。
    pub fn set_analysis_typed(
        &self,
        id: i64,
        description: &str,
        tags: &[String],
        project_type: &str,
        source: &str,
    ) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE projects SET description=?1, tags=?2, project_type=?3, ai_source=?4,
                 status='ok', analyzed_at=?5, updated_at=?5 WHERE id=?6",
                params![description, to_json(tags), project_type, source, now(), id],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// 已有 AI 说明但类型为空的项目（用于补分类），返回 (id, description, tags)。
    pub fn untyped_analyzed(&self, limit: u32) -> Result<Vec<(i64, String, Vec<String>)>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, COALESCE(description,''), tags FROM projects
                 WHERE status='ok' AND COALESCE(project_type,'')='' ORDER BY id LIMIT ?1",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![limit], |r| {
                let tags: String = r.get(2)?;
                Ok((r.get(0)?, r.get(1)?, json_vec(&tags)))
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        Ok(rows)
    }

    /// 设置项目类型（用于补分类）。
    pub fn set_project_type(&self, id: i64, project_type: &str) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE projects SET project_type=?1, updated_at=?2 WHERE id=?3",
                params![project_type, now(), id],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// 语言与类型的分布统计，供筛选器展示可用选项与数量。
    pub fn facets(&self) -> Result<(Vec<(String, usize)>, Vec<(String, usize)>), String> {
        let mut langs: std::collections::BTreeMap<String, usize> = Default::default();
        let mut stmt = self
            .conn
            .prepare("SELECT language, COUNT(*) FROM projects GROUP BY language")
            .map_err(|e| e.to_string())?;
        for row in stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as usize)))
            .map_err(|e| e.to_string())?
        {
            let (k, v) = row.map_err(|e| e.to_string())?;
            if !k.is_empty() {
                langs.insert(k, v);
            }
        }
        let mut types: std::collections::BTreeMap<String, usize> = Default::default();
        let mut stmt = self
            .conn
            .prepare(
                "SELECT project_type, COUNT(*) FROM projects
                 WHERE COALESCE(project_type,'')<>'' GROUP BY project_type",
            )
            .map_err(|e| e.to_string())?;
        for row in stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as usize)))
            .map_err(|e| e.to_string())?
        {
            let (k, v) = row.map_err(|e| e.to_string())?;
            types.insert(k, v);
        }
        Ok((langs.into_iter().collect(), types.into_iter().collect()))
    }

    pub fn set_status(&self, id: i64, status: &str) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE projects SET status=?1, updated_at=?2 WHERE id=?3",
                params![status, now(), id],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn update_notes(&self, id: i64, notes: Option<&str>) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE projects SET notes=?1, updated_at=?2 WHERE id=?3",
                params![notes, now(), id],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// 手动编辑标签（覆盖 AI 标签）。
    pub fn update_tags(&self, id: i64, tags: &[String]) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE projects SET tags=?1, updated_at=?2 WHERE id=?3",
                params![to_json(tags), now(), id],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn delete(&self, id: i64) -> Result<(), String> {
        self.conn
            .execute("DELETE FROM projects WHERE id=?1", params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// 等待 AI 分析的项目 id（排除正在分析中的）。
    pub fn pending_ids(&self, limit: u32) -> Result<Vec<i64>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM projects WHERE status='pending' ORDER BY id LIMIT ?1")
            .map_err(|e| e.to_string())?;
        let ids = stmt
            .query_map(params![limit], |r| r.get(0))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        Ok(ids)
    }

    /// 全部重新分析：把所有非候选项目重置为 pending（保留用户备注与手动标签）。
    /// 候选项目（candidate）保持不动，避免误分析未确认的目录。
    pub fn reset_all_for_reanalysis(&self) -> Result<usize, String> {
        let n = self
            .conn
            .execute(
                "UPDATE projects SET status='pending', updated_at=?1 WHERE status!='candidate'",
                params![now()],
            )
            .map_err(|e| e.to_string())?;
        Ok(n)
    }

    /// 清理磁盘上已不存在的项目，返回删除数量。
    pub fn prune_missing(&self) -> Result<usize, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, path FROM projects")
            .map_err(|e| e.to_string())?;
        let rows: Vec<(i64, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(|e| e.to_string())?
            .collect::<Result<_, _>>()
            .map_err(|e| e.to_string())?;
        let mut removed = 0;
        for (id, path) in rows {
            if !Path::new(&path).exists() {
                self.conn
                    .execute("DELETE FROM projects WHERE id=?1", params![id])
                    .map_err(|e| e.to_string())?;
                removed += 1;
            }
        }
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(path: &str, status: &str) -> NewProject {
        NewProject {
            path: path.into(),
            name: "demo".into(),
            language: "Node.js".into(),
            status: status.into(),
            deps: vec!["express".into()],
            commits: vec!["init".into()],
            readme: Some("demo readme".into()),
            git_remote: None,
            structure: vec!["src/".into()],
            last_commit_at: Some(1000),
        }
    }

    #[test]
    fn upsert_then_list() {
        let db = Db::open_in_memory().unwrap();
        db.upsert(&sample(r"C:\a", "pending")).unwrap();
        let all = db.list().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].name, "demo");
        assert_eq!(all[0].deps, vec!["express"]);
        assert_eq!(all[0].status, "pending");
    }

    #[test]
    fn upsert_is_idempotent_by_path() {
        let db = Db::open_in_memory().unwrap();
        db.upsert(&sample(r"C:\a", "pending")).unwrap();
        db.upsert(&sample(r"C:\a", "pending")).unwrap();
        assert_eq!(db.list().unwrap().len(), 1);
    }

    #[test]
    fn conflict_preserves_analysis_and_notes() {
        let db = Db::open_in_memory().unwrap();
        db.upsert(&sample(r"C:\a", "pending")).unwrap();
        let id = db.get_id_by_path(r"C:\a").unwrap().unwrap();
        db.set_analysis_typed(id, "一个演示项目", &["演示".into()], "Web 应用", "openai-compatible")
            .unwrap();
        db.update_notes(id, Some("我自己的备注")).unwrap();

        let mut again = sample(r"C:\a", "pending");
        again.deps = vec!["express".into(), "axios".into()];
        db.upsert(&again).unwrap();

        let row = db.get(id).unwrap().unwrap();
        assert_eq!(row.description.as_deref(), Some("一个演示项目"));
        assert_eq!(row.tags, vec!["演示"]);
        assert_eq!(row.notes.as_deref(), Some("我自己的备注"));
        assert_eq!(row.status, "ok");
        assert_eq!(row.deps, vec!["express", "axios"]);
    }

    #[test]
    fn candidate_upgrades_when_scan_finds_more() {
        let db = Db::open_in_memory().unwrap();
        db.upsert(&sample(r"C:\a", "candidate")).unwrap();
        db.upsert(&sample(r"C:\a", "pending")).unwrap();
        assert_eq!(db.list().unwrap()[0].status, "pending");
    }

    #[test]
    fn analyzed_status_is_not_downgraded() {
        let db = Db::open_in_memory().unwrap();
        db.upsert(&sample(r"C:\a", "pending")).unwrap();
        let id = db.get_id_by_path(r"C:\a").unwrap().unwrap();
        db.set_analysis_typed(id, "d", &[], "命令行工具", "x").unwrap();
        db.upsert(&sample(r"C:\a", "candidate")).unwrap();
        assert_eq!(db.list().unwrap()[0].status, "ok");
    }

    #[test]
    fn update_notes_and_tags() {
        let db = Db::open_in_memory().unwrap();
        db.upsert(&sample(r"C:\a", "pending")).unwrap();
        let id = db.get_id_by_path(r"C:\a").unwrap().unwrap();
        db.update_notes(id, Some("notes")).unwrap();
        db.update_tags(id, &["a".into(), "b".into()]).unwrap();
        let row = db.get(id).unwrap().unwrap();
        assert_eq!(row.notes.as_deref(), Some("notes"));
        assert_eq!(row.tags, vec!["a", "b"]);
    }

    #[test]
    fn pending_ids_and_status_flow() {
        let db = Db::open_in_memory().unwrap();
        db.upsert(&sample(r"C:\a", "pending")).unwrap();
        db.upsert(&sample(r"C:\b", "ok")).unwrap();
        db.upsert(&sample(r"C:\c", "candidate")).unwrap();
        assert_eq!(db.pending_ids(10).unwrap(), vec![1]);
        let id = db.pending_ids(10).unwrap()[0];
        db.set_status(id, "analyzing").unwrap();
        assert!(db.pending_ids(10).unwrap().is_empty());
        db.set_status(id, "failed").unwrap();
        assert_eq!(db.get(id).unwrap().unwrap().status, "failed");
    }

    #[test]
    fn prune_missing_removes_gone_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("proj");
        std::fs::create_dir_all(&dir).unwrap();
        let db = Db::open_in_memory().unwrap();
        db.upsert(&sample(dir.to_string_lossy().as_ref(), "pending"))
            .unwrap();
        db.upsert(&sample(r"C:\gone", "pending")).unwrap();
        assert_eq!(db.prune_missing().unwrap(), 1); // C:\gone 不存在
        assert_eq!(db.list().unwrap().len(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(db.prune_missing().unwrap(), 1);
        assert!(db.list().unwrap().is_empty());
    }

    #[test]
    fn delete_project() {
        let db = Db::open_in_memory().unwrap();
        db.upsert(&sample(r"C:\a", "pending")).unwrap();
        let id = db.get_id_by_path(r"C:\a").unwrap().unwrap();
        db.delete(id).unwrap();
        assert!(db.get(id).unwrap().is_none());
    }

    #[test]
    fn open_real_file_db() {
        let tmp = tempfile::tempdir().unwrap();
        let db = Db::open(&tmp.path().join("sub/projects.db")).unwrap();
        db.upsert(&sample(r"C:\a", "pending")).unwrap();
        assert_eq!(db.list().unwrap().len(), 1);
    }

    #[test]
    fn missing_project_returns_none() {
        let db = Db::open_in_memory().unwrap();
        assert!(db.get(999).unwrap().is_none());
        assert!(db.get_id_by_path(r"C:\nope").unwrap().is_none());
    }

    #[test]
    fn reset_all_for_reanalysis_moves_done_back_to_pending() {
        let db = Db::open_in_memory().unwrap();
        db.upsert(&sample(r"C:\a", "pending")).unwrap();
        db.upsert(&sample(r"C:\b", "pending")).unwrap();
        let id_a = db.get_id_by_path(r"C:\a").unwrap().unwrap();
        let id_b = db.get_id_by_path(r"C:\b").unwrap().unwrap();
        db.set_analysis_typed(id_a, "已完成", &["t".into()], "库/框架", "x").unwrap();
        db.set_status(id_b, "failed").unwrap();

        let n = db.reset_all_for_reanalysis().unwrap();
        assert_eq!(n, 2);
        assert_eq!(db.get(id_a).unwrap().unwrap().status, "pending");
        assert_eq!(db.get(id_b).unwrap().unwrap().status, "pending");
        // 已有结论保留，供重新分析失败时兜底
        assert_eq!(
            db.get(id_a).unwrap().unwrap().description.as_deref(),
            Some("已完成")
        );
    }

    #[test]
    fn reset_all_keeps_candidates_untouched() {
        let db = Db::open_in_memory().unwrap();
        db.upsert(&sample(r"C:\c", "candidate")).unwrap();
        db.upsert(&sample(r"C:\d", "pending")).unwrap();
        let n = db.reset_all_for_reanalysis().unwrap();
        assert_eq!(n, 1); // 只重置了 pending 那条
        let id_c = db.get_id_by_path(r"C:\c").unwrap().unwrap();
        assert_eq!(db.get(id_c).unwrap().unwrap().status, "candidate");
        assert!(db.pending_ids(10).unwrap().len() == 1);
    }

    #[test]
    fn reset_all_preserves_user_notes_and_tags() {
        let db = Db::open_in_memory().unwrap();
        db.upsert(&sample(r"C:\e", "pending")).unwrap();
        let id = db.get_id_by_path(r"C:\e").unwrap().unwrap();
        db.update_notes(id, Some("我的备注")).unwrap();
        db.update_tags(id, &["手动标签".into()]).unwrap();
        db.reset_all_for_reanalysis().unwrap();
        let row = db.get(id).unwrap().unwrap();
        assert_eq!(row.notes.as_deref(), Some("我的备注"));
        assert_eq!(row.tags, vec!["手动标签"]);
    }
}

#[cfg(test)]
mod filter_tests {
    use super::*;

    fn proj(path: &str, lang: &str) -> NewProject {
        NewProject {
            path: path.into(),
            name: path.into(),
            language: lang.into(),
            status: "pending".into(),
            deps: vec![],
            commits: vec![],
            readme: None,
            git_remote: None,
            structure: vec![],
            last_commit_at: None,
        }
    }

    #[test]
    fn project_type_roundtrip() {
        let db = Db::open_in_memory().unwrap();
        db.upsert(&proj(r"C:\a", "Rust")).unwrap();
        let id = db.get_id_by_path(r"C:\a").unwrap().unwrap();
        assert_eq!(db.get(id).unwrap().unwrap().project_type, "");

        db.set_analysis_typed(id, "工具", &["cli".into()], "命令行工具", "ai")
            .unwrap();
        let row = db.get(id).unwrap().unwrap();
        assert_eq!(row.project_type, "命令行工具");
        assert_eq!(row.status, "ok");
    }

    #[test]
    fn facets_count_languages_and_types() {
        let db = Db::open_in_memory().unwrap();
        db.upsert(&proj(r"C:\a", "Rust")).unwrap();
        db.upsert(&proj(r"C:\b", "Rust")).unwrap();
        db.upsert(&proj(r"C:\c", "Node.js")).unwrap();
        let ida = db.get_id_by_path(r"C:\a").unwrap().unwrap();
        let idb = db.get_id_by_path(r"C:\b").unwrap().unwrap();
        db.set_analysis_typed(ida, "d", &[], "命令行工具", "x").unwrap();
        db.set_analysis_typed(idb, "d", &[], "Web 应用", "x").unwrap();

        let (langs, types) = db.facets().unwrap();
        assert_eq!(
            langs,
            vec![("Node.js".to_string(), 1), ("Rust".to_string(), 2)]
        );
        assert_eq!(
            types,
            vec![("Web 应用".to_string(), 1), ("命令行工具".to_string(), 1)]
        );
    }

    #[test]
    fn facets_exclude_empty_values() {
        let db = Db::open_in_memory().unwrap();
        db.upsert(&proj(r"C:\a", "")).unwrap();
        let (langs, types) = db.facets().unwrap();
        assert!(langs.is_empty());
        assert!(types.is_empty());
    }

    #[test]
    fn untyped_analyzed_lists_only_done_without_type() {
        let db = Db::open_in_memory().unwrap();
        db.upsert(&proj(r"C:\done-notype", "Rust")).unwrap();
        db.upsert(&proj(r"C:\done-typed", "Rust")).unwrap();
        db.upsert(&proj(r"C:\pending", "Rust")).unwrap();

        let a = db.get_id_by_path(r"C:\done-notype").unwrap().unwrap();
        let b = db.get_id_by_path(r"C:\done-typed").unwrap().unwrap();
        db.set_analysis_typed(a, "有说明无类型", &["t".into()], "", "x")
            .unwrap();
        db.set_analysis_typed(b, "有说明有类型", &[], "库/框架", "x")
            .unwrap();

        let rows = db.untyped_analyzed(100).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, a);
        assert_eq!(rows[0].1, "有说明无类型");
        assert_eq!(rows[0].2, vec!["t"]);
    }

    #[test]
    fn set_project_type_updates_existing_row() {
        let db = Db::open_in_memory().unwrap();
        db.upsert(&proj(r"C:\a", "Python")).unwrap();
        let id = db.get_id_by_path(r"C:\a").unwrap().unwrap();
        db.set_project_type(id, "爬虫/数据采集").unwrap();
        assert_eq!(
            db.get(id).unwrap().unwrap().project_type,
            "爬虫/数据采集"
        );
    }

    #[test]
    fn migration_adds_project_type_to_legacy_table() {
        // 模拟老库：建一个没有 project_type 列的表，再跑 open 的迁移路径
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("legacy.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE projects (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    path TEXT NOT NULL UNIQUE,
                    name TEXT NOT NULL,
                    language TEXT NOT NULL DEFAULT '',
                    status TEXT NOT NULL DEFAULT 'pending',
                    description TEXT, tags TEXT NOT NULL DEFAULT '[]', notes TEXT,
                    deps TEXT NOT NULL DEFAULT '[]', commits TEXT NOT NULL DEFAULT '[]',
                    readme TEXT, git_remote TEXT, structure TEXT NOT NULL DEFAULT '[]',
                    last_commit_at INTEGER, analyzed_at INTEGER, ai_source TEXT,
                    updated_at INTEGER NOT NULL DEFAULT 0);
                 INSERT INTO projects(path,name,language,status)
                    VALUES('C:/old','old','Rust','pending');",
            )
            .unwrap();
        }
        // open() 会建表 + migrate，应补上 project_type 且不丢老数据
        let db = Db::open(&path).unwrap();
        let rows = db.list().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "old");
        assert_eq!(rows[0].project_type, "");
        // 并且新列可用
        db.set_project_type(rows[0].id, "库/框架").unwrap();
        assert_eq!(db.get(rows[0].id).unwrap().unwrap().project_type, "库/框架");
    }
}
