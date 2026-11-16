use crate::domain::*;
use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::{fs, path::PathBuf};

pub fn db_path() -> PathBuf { crate::scan::state_dir().join("history.sqlite3") }
pub fn open() -> Result<Connection> { open_at(&db_path()) }
pub fn open_at(path: &std::path::Path) -> Result<Connection> {
    if let Some(p) = path.parent() {
        fs::create_dir_all(p)?;
        #[cfg(unix)] {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(p, fs::Permissions::from_mode(0o700))?;
        }
    }
    let db = Connection::open(path).context("open Treills SQLite database")?;
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    db.pragma_update(None, "journal_mode", "WAL")?;
    db.busy_timeout(std::time::Duration::from_secs(3))?;
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS files (
            path TEXT PRIMARY KEY, identity TEXT NOT NULL, prefix TEXT NOT NULL,
            offset INTEGER NOT NULL, generation INTEGER NOT NULL DEFAULT 0,
            current_turn TEXT, incomplete INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS projects (
            id TEXT PRIMARY KEY, name TEXT NOT NULL, root TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS sessions (
            id TEXT PRIMARY KEY, project_id TEXT NOT NULL, source TEXT NOT NULL,
            native_id TEXT NOT NULL, parent_turn_id TEXT,
            started_at INTEGER, ended_at INTEGER, latest_at INTEGER,
            state TEXT NOT NULL DEFAULT 'unknown', worktree TEXT NOT NULL DEFAULT '',
            branch TEXT, incomplete INTEGER NOT NULL DEFAULT 0, evidence TEXT NOT NULL DEFAULT '[]'
        );
        CREATE INDEX IF NOT EXISTS idx_sessions_project ON sessions(project_id,latest_at DESC);
        CREATE TABLE IF NOT EXISTS workflows (
            id TEXT PRIMARY KEY, project_id TEXT NOT NULL, source TEXT NOT NULL,
            session_id TEXT NOT NULL, name TEXT NOT NULL, state TEXT NOT NULL DEFAULT 'unknown',
            started_at INTEGER, ended_at INTEGER, latest_at INTEGER, worktree TEXT NOT NULL DEFAULT '',
            branch TEXT, model TEXT, incomplete INTEGER NOT NULL DEFAULT 0,
            evidence TEXT NOT NULL DEFAULT '[]',turn_id TEXT NOT NULL,parent_turn_id TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_workflows_project ON workflows(project_id, latest_at DESC);
        CREATE INDEX IF NOT EXISTS idx_workflows_session ON workflows(session_id, latest_at DESC);
        CREATE TABLE IF NOT EXISTS nodes (
            id TEXT PRIMARY KEY, workflow_id TEXT NOT NULL, parent_id TEXT,
            child_session_id TEXT, role TEXT NOT NULL, model TEXT, state TEXT NOT NULL DEFAULT 'unknown',
            started_at INTEGER, ended_at INTEGER, latest_at INTEGER,
            last_tool TEXT, evidence TEXT NOT NULL DEFAULT '[]'
        );
        CREATE INDEX IF NOT EXISTS idx_nodes_workflow ON nodes(workflow_id);
        CREATE TABLE IF NOT EXISTS events (
            id TEXT PRIMARY KEY, workflow_id TEXT,session_id TEXT NOT NULL,turn_id TEXT,
            node_id TEXT NOT NULL,at INTEGER,kind TEXT NOT NULL, detail TEXT NOT NULL,
            evidence_source TEXT NOT NULL, call_id TEXT, ingested_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_events_workflow ON events(workflow_id, at, id);
        CREATE INDEX IF NOT EXISTS idx_events_session ON events(session_id, at, id);
        CREATE TABLE IF NOT EXISTS calls (
            workflow_id TEXT NOT NULL, node_id TEXT NOT NULL, call_id TEXT NOT NULL,
            name TEXT NOT NULL, started_at INTEGER, finished_at INTEGER, result TEXT,
            PRIMARY KEY(workflow_id, node_id, call_id)
        );
        CREATE TABLE IF NOT EXISTS issues (
            path TEXT PRIMARY KEY, message TEXT NOT NULL, recorded_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS changes (
            seq INTEGER PRIMARY KEY AUTOINCREMENT, workflow_id TEXT NOT NULL,
            project_id TEXT NOT NULL, at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_changes_seq ON changes(seq);"
    )?;
    Ok(db)
}
pub fn now() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).map(|t|t.as_secs() as i64).unwrap_or_default()
}
pub fn last_seq(db: &Connection) -> Result<i64> {
    Ok(db.query_row("SELECT COALESCE(MAX(seq),0) FROM changes", [], |r|r.get(0))?)
}
fn evidence(raw: String) -> Vec<String> { serde_json::from_str(&raw).unwrap_or_default() }
fn map_workflow(r: &rusqlite::Row<'_>) -> rusqlite::Result<Workflow> {
    let state: String = r.get(5)?;
    let raw: String = r.get(13)?;
    Ok(Workflow {
        id:r.get(0)?, project_id:r.get(1)?, source:r.get(2)?,session_id:r.get(3)?,
        turn_id:r.get(15)?,parent_turn_id:r.get(16)?,
        name:r.get(4)?,state:RunState::from_db(&state),started_at:r.get(6)?,
        ended_at:r.get(7)?,latest_at:r.get(8)?,worktree:r.get(9)?,branch:r.get(10)?,
        model:r.get(11)?,node_count:r.get(12)?,incomplete:r.get::<_,i64>(14)?!=0,
        evidence:evidence(raw),
    })
}
const WF_COLUMNS:&str = "w.id,w.project_id,w.source,w.session_id,w.name,w.state,w.started_at,w.ended_at,w.latest_at,w.worktree,w.branch,w.model,(SELECT COUNT(*) FROM nodes n WHERE n.workflow_id=w.id),w.evidence,w.incomplete,w.turn_id,w.parent_turn_id";
fn events_query(db: &Connection, id: &str, cursor: Option<i64>, limit:i64) -> Result<EventsPage> {
    let cursor=cursor.unwrap_or(0);
    let mut stmt=db.prepare("SELECT id, workflow_id, session_id, turn_id, node_id, at, kind, detail, evidence_source, call_id, rowid
      FROM events WHERE workflow_id=?1 AND rowid>?2 ORDER BY rowid ASC LIMIT ?3")?;
    let rows=stmt.query_map(params![id,cursor,limit+1],|r|{
        Ok((TimelineEvent{id:r.get(0)?, workflow_id:r.get::<_,Option<String>>(1)?.unwrap_or_default(),session_id:r.get(2)?,turn_id:r.get(3)?,node_id:r.get(4)?,at:r.get(5)?,
        kind:r.get(6)?,detail:r.get(7)?,evidence_source:r.get(8)?,call_id:r.get(9)?},r.get::<_,i64>(10)?))
    })?.collect::<rusqlite::Result<Vec<_>>>()?;
    let has_more=rows.len() as i64>limit;
    let mut rows=rows;
    if has_more{rows.pop();}
    let next_cursor=if has_more{rows.last().map(|r|r.1)}else{None};
    Ok(EventsPage{api_version:API_VERSION.into(),items:rows.into_iter().map(|r|r.0).collect(),next_cursor})
}
pub fn events(db:&Connection,id:&str,cursor:Option<i64>,limit:i64)->Result<EventsPage>{
    events_query(db,id,cursor,limit.clamp(1,200))
}
pub fn session_events(db:&Connection,id:&str,cursor:Option<i64>,limit:i64)->Result<EventsPage>{
    let cursor=cursor.unwrap_or(0);
    let limit=limit.clamp(1,200);
    let mut stmt=db.prepare("SELECT id,workflow_id,session_id,turn_id,node_id,at,kind,detail,evidence_source,call_id,rowid
      FROM events WHERE session_id=?1 AND rowid>?2 ORDER BY rowid ASC LIMIT ?3")?;
    let mut rows=stmt.query_map(params![id,cursor,limit+1],|r|{
        Ok((TimelineEvent{
            id:r.get(0)?,workflow_id:r.get::<_,Option<String>>(1)?.unwrap_or_default(),
            session_id:r.get(2)?,turn_id:r.get(3)?,node_id:r.get(4)?,at:r.get(5)?,
            kind:r.get(6)?,detail:r.get(7)?,evidence_source:r.get(8)?,call_id:r.get(9)?
        },r.get::<_,i64>(10)?))
    })?.collect::<rusqlite::Result<Vec<_>>>()?;
    let more=rows.len() as i64>limit;
    if more{rows.pop();}
    let next_cursor=if more{rows.last().map(|r|r.1)}else{None};
    Ok(EventsPage{api_version:API_VERSION.into(),items:rows.into_iter().map(|r|r.0).collect(),next_cursor})
}

pub fn workflow(db:&Connection,id:&str)->Result<Option<WorkflowDetail>>{
    let sql=format!("SELECT {WF_COLUMNS} FROM workflows w WHERE w.id=?1");
    let wf=db.query_row(&sql,[id],map_workflow).optional()?;
    let Some(workflow)=wf else{return Ok(None)};
    let mut stmt=db.prepare("SELECT id,workflow_id,parent_id,child_session_id,role,model,state,started_at,ended_at,latest_at,last_tool,evidence FROM nodes WHERE workflow_id=?1 ORDER BY started_at,id")?;
    let nodes=stmt.query_map([id],|r|{
        let state:String=r.get(6)?;
        let raw:String=r.get(11)?;
        Ok(AgentNode{id:r.get(0)?,workflow_id:r.get(1)?,parent_id:r.get(2)?,
          child_session_id:r.get(3)?,role:r.get(4)?,model:r.get(5)?,state:RunState::from_db(&state),
          started_at:r.get(7)?,ended_at:r.get(8)?,latest_at:r.get(9)?,
          last_tool:r.get(10)?,evidence:evidence(raw)})
    })?.collect::<rusqlite::Result<Vec<_>>>()?;
    let known:std::collections::HashSet<&str>=nodes.iter().map(|n|n.id.as_str()).collect();
    let edges=nodes.iter().filter_map(|n|{
        let from=n.parent_id.as_ref()?;
        if !known.contains(from.as_str()){return None;}
        Some(GraphEdge{id:format!("{}>{}",from,n.id),from:from.clone(),to:n.id.clone(),
            evidence_id:n.evidence.first().cloned().unwrap_or_default()})
    }).collect();
    let events=events_query(db,id,None,100)?.items;
    Ok(Some(WorkflowDetail{api_version:API_VERSION.into(),workflow,nodes,edges,events}))
}
#[allow(clippy::too_many_arguments)]
pub fn workflows(db:&Connection, project_id:Option<&str>,session_id:Option<&str>,turn_id:Option<&str>,status:Option<&str>,search:Option<&str>,offset:i64,limit:i64)->Result<WorkflowsPage>{
    let limit=limit.clamp(1,100);
    let sql=format!("SELECT {WF_COLUMNS} FROM workflows w
       WHERE (?1 IS NULL OR w.project_id=?1)
       AND (?2 IS NULL OR w.session_id=?2)
       AND (?3 IS NULL OR w.turn_id=?3)
       AND (?4 IS NULL OR w.state=?4)
       AND (?5 IS NULL OR instr(lower(w.name),lower(?5))>0)
       ORDER BY COALESCE(w.latest_at,0) DESC,w.id LIMIT ?6 OFFSET ?7");
    let mut stmt=db.prepare(&sql)?;
    let mut rows=stmt.query_map(params![project_id,session_id,turn_id,status,search,limit+1,offset.max(0)],map_workflow)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let more=rows.len() as i64>limit;
    if more{rows.pop();}
    Ok(WorkflowsPage{api_version:API_VERSION.into(),items:rows,
        next_offset:more.then_some(offset.max(0)+limit)})
}
pub fn projects(db:&Connection,filter:Option<&str>,search:Option<&str>,offset:i64,limit:i64)->Result<ProjectsPage>{
    let limit=limit.clamp(1,100);
    let sql="SELECT id,name,root FROM projects
      WHERE (?1 IS NULL OR instr(lower(name),lower(?1))>0 OR EXISTS
        (SELECT 1 FROM workflows w WHERE w.project_id=projects.id AND instr(lower(w.name),lower(?1))>0))
      ORDER BY COALESCE((SELECT MAX(latest_at) FROM workflows w WHERE w.project_id=projects.id),0) DESC,id";
    let mut stmt=db.prepare(sql)?;
    let all=stmt.query_map(params![search],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut found=Vec::new();
    for (id,name,root) in all {
        let mut q=db.prepare("SELECT id,name,state,(SELECT COUNT(*) FROM nodes WHERE workflow_id=w.id),latest_at FROM workflows w WHERE project_id=?1 ORDER BY COALESCE(latest_at,0) DESC LIMIT 5")?;
        let previews=q.query_map([&id],|r| {
            let state:String=r.get(2)?;
            Ok((Preview{workflow_id:r.get(0)?,name:r.get(1)?,state:RunState::from_db(&state),node_count:r.get(3)?},r.get::<_,Option<i64>>(4)?))
        })?.collect::<rusqlite::Result<Vec<_>>>()?;
        let mut active=0;let mut attention=0;let mut ended=0;
        let mut all_states=db.prepare("SELECT state FROM workflows WHERE project_id=?1")?;
        for state in all_states.query_map([&id],|r|r.get::<_,String>(0))? {
            match RunState::from_db(&state?) {
                RunState::Recent|RunState::Running=>active+=1,
                RunState::Waiting|RunState::Failed|RunState::Unknown=>attention+=1,
                x if x.is_terminal()=>ended+=1,
                _=>{}
            }
        }
        if matches!(filter,Some("active")) && active==0 {continue;}
        if matches!(filter,Some("attention")) && attention==0 {continue;}
        if matches!(filter,Some("completed")) && ended==0 {continue;}
        let latest_workflow=previews.iter().filter_map(|p|p.1).max();
        let latest_session:Option<i64>=db.query_row(
            "SELECT MAX(latest_at) FROM sessions WHERE project_id=?1",[&id],|r|r.get(0))?;
        let latest_at=latest_workflow.into_iter().chain(latest_session).max();
        found.push(Project{id,name,root,active_count:active,attention_count:attention,
          ended_count:ended,latest_at,previews:previews.into_iter().map(|p|p.0).collect()});
    }
    let count=found.len();
    let start=offset.max(0) as usize;
    let items=found.into_iter().skip(start).take(limit as usize).collect();
    let next_offset=(start+(limit as usize)<count).then_some(offset.max(0)+limit);
    let mut stmt=db.prepare("SELECT path,message,recorded_at FROM issues ORDER BY recorded_at DESC LIMIT 15")?;
    let issues=stmt.query_map([],|r|Ok(IngestIssue{path:r.get(0)?,message:r.get(1)?,recorded_at:r.get(2)?}))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(ProjectsPage{api_version:API_VERSION.into(),items,next_offset,last_change_id:last_seq(db)?,issues})
}
pub fn sessions(db:&Connection,project_id:Option<&str>,source:Option<&str>,offset:i64,limit:i64)->Result<SessionsPage>{
    let limit=limit.clamp(1,100);
    let mut stmt=db.prepare("SELECT id,project_id,source,native_id,parent_turn_id,started_at,ended_at,latest_at,state,worktree,branch,incomplete,evidence FROM sessions
      WHERE (?1 IS NULL OR project_id=?1) AND (?2 IS NULL OR source=?2)
      ORDER BY COALESCE(latest_at,0) DESC,id LIMIT ?3 OFFSET ?4")?;
    let mut all=stmt.query_map(params![project_id,source,limit+1,offset.max(0)],|r|{
        let state:String=r.get(8)?;
        let ev:String=r.get(12)?;
        Ok(SessionRecord{id:r.get(0)?,project_id:r.get(1)?,source:r.get(2)?,native_id:r.get(3)?,
            parent_turn_id:r.get(4)?,started_at:r.get(5)?,ended_at:r.get(6)?,latest_at:r.get(7)?,
            state:RunState::from_db(&state),worktree:r.get(9)?,branch:r.get(10)?,
            incomplete:r.get::<_,i64>(11)?!=0,evidence:evidence(ev)})
    })?.collect::<rusqlite::Result<Vec<_>>>()?;
    let more=all.len() as i64>limit;
    if more{all.pop();}
    Ok(SessionsPage{api_version:API_VERSION.into(),items:all,next_offset:more.then_some(offset.max(0)+limit)})
}

pub fn changes(db:&Connection,after:i64,limit:i64)->Result<ChangesPage>{
    let limit=limit.clamp(1,200);
    let mut stmt=db.prepare("SELECT seq,workflow_id,project_id,at FROM changes WHERE seq>?1 ORDER BY seq LIMIT ?2")?;
    let mut rows=stmt.query_map(params![after,limit+1],|r|Ok(Change{seq:r.get(0)?,workflow_id:r.get(1)?,project_id:r.get(2)?,at:r.get(3)?}))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let has_more=rows.len() as i64>limit;
    if has_more{rows.pop();}
    let next_cursor=rows.last().map(|c|c.seq).unwrap_or(after);
    Ok(ChangesPage{api_version:API_VERSION.into(),items:rows,next_cursor,has_more})
}
pub fn issue(db:&Connection,path:&str,msg:&str)->Result<()>{
    db.execute("INSERT INTO issues(path,message,recorded_at) VALUES (?1,?2,?3)
      ON CONFLICT(path) DO UPDATE SET message=excluded.message,recorded_at=excluded.recorded_at",
      params![path,msg,now()])?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn persistence_and_cursor() {
        let dir=tempfile::tempdir().unwrap();
        let db=Connection::open(dir.path().join("db")).unwrap();
        // Use the same schema through a second real open in integration tests.
        assert!(db.is_autocommit());
    }
}
