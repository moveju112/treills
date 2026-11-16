use crate::model::Session;
use anyhow::Result;
use serde_json::Value;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use walkdir::WalkDir;

fn home() -> PathBuf { dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")) }
pub fn state_dir() -> PathBuf { dirs::state_dir().unwrap_or_else(|| home().join(".local/state")).join("treills") }
pub fn event_file() -> PathBuf { state_dir().join("events.jsonl") }

fn tail_jsonl(file: &Path, limit_bytes: u64, mut handle: impl FnMut(Value)) -> Result<()> {
    let mut f=File::open(file)?;
    let size=f.metadata()?.len();
    let start=size.saturating_sub(limit_bytes);
    f.seek(SeekFrom::Start(start))?;
    let mut reader=BufReader::new(f);
    if start > 0 { let mut skip=String::new(); reader.read_line(&mut skip)?; }
    let mut buf=String::new();
    loop {
        buf.clear();
        if reader.read_line(&mut buf)? == 0 { break; }
        if let Ok(v)=serde_json::from_str::<Value>(buf.trim_end()) { handle(v); }
    }
    Ok(())
}
fn latest_epoch(file: &Path) -> u64 {
    fs::metadata(file).ok().and_then(|m| m.modified().ok())
        .and_then(|v|v.duration_since(UNIX_EPOCH).ok()).map(|d|d.as_secs()).unwrap_or(0)
}
fn session_id(file: &Path) -> String {
    file.file_stem().and_then(|s|s.to_str()).unwrap_or("unknown").to_owned()
}
pub fn scan() -> Vec<Session> {
    let mut sessions:HashMap<String,Session>=HashMap::new();
    for (source,root) in [
        ("claude",home().join(".claude/projects")),
        ("codex",home().join(".codex/sessions"))
    ] {
        if !root.exists(){continue;}
        for entry in WalkDir::new(root).max_depth(7).into_iter().filter_map(|e| e.ok()) {
            if !entry.file_type().is_file() || entry.path().extension().and_then(|v|v.to_str())!=Some("jsonl") {continue;}
            let path=entry.path();
            let name=session_id(path);
            if source=="claude" && name.starts_with("agent-"){continue;}
            let id=if source=="codex" {
                // Codex rollout-... contains an embedded UUID; retain the filename as a stable fallback.
                name.clone()
            }else{name.clone()};
            let key=format!("{source}:{id}");
            let session=sessions.entry(key).or_insert_with(||Session::new(&id,source));
            session.file=path.to_path_buf();
            session.last_epoch=latest_epoch(path);
            let _=tail_jsonl(path,512*1024,|v|session.apply(&v));
        }
    }
    let file=event_file();
    if file.exists() {
        let _=tail_jsonl(&file,2*1024*1024,|v| {
            let source=v.get("source").and_then(Value::as_str).unwrap_or("hook");
            let id=v.get("session_id").and_then(Value::as_str).unwrap_or("unknown");
            let key=format!("{source}:{id}");
            let session=sessions.entry(key).or_insert_with(||Session::new(id,source));
            session.last_epoch=session.last_epoch.max(latest_epoch(&file));
            session.apply(&v);
        });
    }
    let mut out:Vec<_>=sessions.into_values().collect();
    out.sort_by(|a,b|b.last_epoch.cmp(&a.last_epoch));
    out.truncate(100);
    out
}
pub fn read_events(follow: bool) -> Result<()> {
    let path=event_file();
    if !path.exists(){anyhow::bail!("no events yet: {}",path.display())}
    if !follow {
        let mut s=String::new();File::open(path)?.read_to_string(&mut s)?;print!("{s}");return Ok(());
    }
    use std::time::Duration;
    let mut offset=0;
    loop {
        let metadata=fs::metadata(&path)?;
        if metadata.len()<offset {offset=0;}
        let mut f=File::open(&path)?;
        f.seek(SeekFrom::Start(offset))?;
        let mut s=String::new();f.read_to_string(&mut s)?;
        offset=metadata.len();
        print!("{s}");
        std::thread::sleep(Duration::from_millis(300));
    }
}
