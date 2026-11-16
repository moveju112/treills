use crate::{collector::{self, Sources}, domain::RunState, model::Session, store};
use anyhow::{Context, Result};
use std::{collections::BTreeMap, fs::File, io::{Read, Seek, SeekFrom}, path::PathBuf, thread, time::Duration};

pub fn state_dir() -> PathBuf {
    dirs::state_dir().unwrap_or_else(||dirs::home_dir().unwrap_or_else(||PathBuf::from(".")).join(".local/state")).join("treills")
}
pub fn event_file() -> PathBuf {state_dir().join("events.jsonl")}
pub fn scan_checked() -> Result<Vec<Session>> {
    let mut db=store::open()?;
    collector::refresh(&mut db,&Sources::default()).context("incremental session collection")?;
    let mut result=Vec::new();
    let mut offset=0;
    loop {
        let page=store::workflows(&db,None,None,None,None,None,offset,100)?;
        for w in page.items {
            let mut session=Session::new(&w.session_id,&w.source);
            session.cwd=w.worktree;
            session.phase=w.state.as_str().into();
            session.ended=w.state.is_terminal();
            session.model=w.model.unwrap_or_default();
            session.last_epoch=w.latest_at.unwrap_or_default() as u64;
            let mut stmt=db.prepare("SELECT name,COUNT(*) FROM calls WHERE workflow_id=?1 AND name<>'' GROUP BY name")?;
            session.tools=stmt.query_map([&w.id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)? as usize)))?
                .collect::<rusqlite::Result<BTreeMap<_,_>>>()?;
            let mut stmt=db.prepare("SELECT detail,COUNT(*) FROM events WHERE workflow_id=?1 AND kind IN ('skill_confirmed','skill_candidate') GROUP BY detail")?;
            session.skills=stmt.query_map([&w.id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)? as usize)))?
                .collect::<rusqlite::Result<BTreeMap<_,_>>>()?;
            let mut stmt=db.prepare("SELECT id,role,state FROM nodes WHERE workflow_id=?1 AND role<>'main'")?;
            for row in stmt.query_map([&w.id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?)))? {
                let (id,role,state)=row?;
                if !RunState::from_db(&state).is_terminal(){session.agents.insert(id,role);}
            }
            let events=store::events(&db,&w.id,None,100)?;
            for e in events.items.into_iter().rev().take(35).rev() {
                session.activity.push_back(format!("{} {}",e.kind,e.detail));
            }
            result.push(session);
        }
        if let Some(next)=page.next_offset{offset=next;}else{break;}
    }
    Ok(result)
}
pub fn read_events(follow:bool)->Result<()> {
    let path=event_file();
    if !path.exists(){anyhow::bail!("no events yet: {}",path.display());}
    if !follow{
        let mut content=String::new();
        File::open(&path)?.read_to_string(&mut content)?;
        print!("{content}");
        return Ok(());
    }
    let mut offset=0;
    loop{
        let metadata=std::fs::metadata(&path)?;
        if metadata.len()<offset{offset=0;}
        let mut file=File::open(&path)?;
        file.seek(SeekFrom::Start(offset))?;
        let mut content=String::new();
        file.read_to_string(&mut content)?;
        offset=metadata.len();
        print!("{content}");
        thread::sleep(Duration::from_millis(300));
    }
}
