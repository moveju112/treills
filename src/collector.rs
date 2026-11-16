use crate::{domain::RunState, scan, store};
use anyhow::Result;
use chrono::DateTime;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File}, io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};
use walkdir::WalkDir;

#[derive(Clone, Debug)]
pub struct Sources {
    pub claude: PathBuf,
    pub codex: PathBuf,
    pub hooks: PathBuf,
}
impl Default for Sources {
    fn default() -> Self {
        let h = dirs::home_dir().unwrap_or_else(||PathBuf::from("."));
        Self {claude:h.join(".claude/projects"),codex:h.join(".codex/sessions"),hooks:scan::event_file()}
    }
}
#[derive(Debug, Default)]
pub struct Report {pub files:usize,pub events:usize,pub issues:usize}

fn digest(s:impl AsRef<[u8]>)->String{format!("{:x}",Sha256::digest(s))}
fn lookup<'a>(v:&'a Value,paths:&[&str])->Option<&'a str>{
    paths.iter().find_map(|p|v.pointer(p).and_then(Value::as_str)).filter(|v|!v.is_empty())
}
fn at(v:&Value)->Option<i64>{
    for path in ["/ts_unix","/timestamp","/ts","/payload/timestamp","/payload/ts","/payload/created_at"]{
        let Some(val)=v.pointer(path) else {continue;};
        if let Some(n)=val.as_i64(){return Some(if n>100_000_000_000{n/1000}else{n});}
        if let Some(s)=val.as_str(){
            if let Ok(dt)=DateTime::parse_from_rfc3339(s){return Some(dt.timestamp());}
            if let Ok(n)=s.parse::<i64>(){return Some(if n>100_000_000_000{n/1000}else{n});}
        }
    }
    None
}
fn identity(file:&Path)->Result<String>{
    let m=fs::metadata(file)?;
    #[cfg(unix)]{use std::os::unix::fs::MetadataExt;Ok(format!("{}:{}",m.dev(),m.ino()))}
    #[cfg(not(unix))]{Ok(format!("{}:{:?}",m.len(),m.created().ok()))}
}
fn prefix(file:&Path)->Result<String>{
    let mut f=File::open(file)?;
    let mut buffer=[0u8;64];
    let n=f.read(&mut buffer)?;
    Ok(digest(&buffer[..n]))
}
fn native_session(file:&Path,source:&str,v:&Value)->Option<String>{
    if source=="hook" {
        return lookup(v,&["/session_id","/payload/session_id"]).map(str::to_owned);
    }
    let stem=file.file_stem().and_then(|s|s.to_str())?;
    if source=="claude" && stem.starts_with("agent-") {
        return file.parent()?.parent()?.file_name()?.to_str().map(str::to_owned);
    }
    if source=="codex" {
        if let Some(id)=lookup(v,&["/payload/id","/session_id"]) {
            if lookup(v,&["/payload/type"])==Some("session_meta"){return Some(id.to_owned());}
        }
        if stem.len()>=36 {
            let tail=&stem[stem.len()-36..];
            if tail.as_bytes().iter().enumerate().all(|(i,c)|[8,13,18,23].contains(&i)&&*c==b'-'||
                ![8,13,18,23].contains(&i)&&c.is_ascii_hexdigit()){return Some(tail.to_owned());}
        }
    }
    Some(stem.into())
}
fn canonical(p:&Path)->PathBuf{p.canonicalize().unwrap_or_else(|_|p.to_path_buf())}
fn project_location(cwd:&str)->(String,String,Option<String>){
    let location=canonical(Path::new(cwd));
    let mut current=Some(location.as_path());
    while let Some(p)=current {
        let dot=p.join(".git");
        if dot.exists(){
            let gitdir=if dot.is_dir(){canonical(&dot)}else{
                fs::read_to_string(&dot).ok()
                    .and_then(|s|s.trim().strip_prefix("gitdir:").map(str::trim).map(|relative|{
                        let g=Path::new(relative);
                        if g.is_absolute(){canonical(g)}else{canonical(&p.join(g))}
                    })).unwrap_or(dot)
            };
            let common=fs::read_to_string(gitdir.join("commondir")).ok()
                .map(|s|{let rel=PathBuf::from(s.trim());if rel.is_absolute(){canonical(&rel)}else{canonical(&gitdir.join(rel))}})
                .unwrap_or(gitdir.clone());
            let root=common.parent().unwrap_or(p).to_string_lossy().into_owned();
            let branch=fs::read_to_string(gitdir.join("HEAD")).ok()
                .and_then(|s|s.trim().strip_prefix("ref: refs/heads/").map(str::to_owned));
            return (root,p.to_string_lossy().into_owned(),branch);
        }
        current=p.parent();
    }
    (location.to_string_lossy().into_owned(),location.to_string_lossy().into_owned(),None)
}
fn project(db:&Connection,cwd:&str,sid:&str)->Result<(String,String,Option<String>)>{
    let (root,worktree,branch)=if cwd.is_empty(){
        (format!("[unresolved] {sid}"),String::new(),None)
    } else {project_location(cwd)};
    let id=format!("p:{}", &digest(root.as_bytes())[..24]);
    let name=Path::new(&root).file_name().and_then(|s|s.to_str()).unwrap_or("Unknown").to_owned();
    db.execute("INSERT INTO projects(id,name,root) VALUES (?1,?2,?3)
       ON CONFLICT(id) DO UPDATE SET name=excluded.name,root=excluded.root",params![id,name,root])?;
    Ok((id,worktree,branch))
}
fn evidence(raw:Option<String>,id:&str)->String{
    let mut values:Vec<String>=raw.and_then(|s|serde_json::from_str(&s).ok()).unwrap_or_default();
    if !values.iter().any(|value|value==id){values.push(id.into());}
    if values.len()>16{values.remove(0);}
    serde_json::to_string(&values).unwrap_or_else(|_|"[]".into())
}
fn state(event:&str,v:&Value,time:Option<i64>)->RunState{
    // Ended is not success. A tool call is not evidence of passing validation.
    if ["SessionEnd","session_end","SubagentStop"].contains(&event){return RunState::Ended;}
    if ["Stop","Notification","permission_requested"].contains(&event){return RunState::Waiting;}
    if ["turn_cancelled","cancelled"].contains(&event){return RunState::Cancelled;}
    if ["turn_failed","failure"].contains(&event){return RunState::Failed;}
    if let Some(value)=lookup(v,&["/result/status","/payload/result/status","/execution_status"]){
        return match value{
            "success"|"passed"=>RunState::Success,
            "failure"|"failed"|"error"=>RunState::Failed,
            "cancelled"|"canceled"=>RunState::Cancelled,
            "waiting"=>RunState::Waiting,
            _=>RunState::Unknown
        }
    }
    match time {
        Some(t) if store::now()-t<120=>RunState::Recent,
        Some(_)=>RunState::Inactive,
        None=>RunState::Unknown
    }
}
fn merge_state(previous:&str,latest:RunState,previous_at:Option<i64>,at:Option<i64>)->String{
    let p=RunState::from_db(previous);
    if p.is_terminal()&&!latest.is_terminal(){return previous.into();}
    if let (Some(a),Some(b))=(previous_at,at){if b<a{return previous.into();}}
    if latest==RunState::Unknown{previous.into()}else{latest.as_str().into()}
}
fn native_turn_start(v:&Value,source:&str,offset:u64,file:&Path)->Option<String>{
    let event=lookup(v,&["/event","/payload/type"]).unwrap_or_default();
    let t=lookup(v,&["/type"]).unwrap_or_default();
    let start=if source=="claude" {
        t=="user" && match v.pointer("/message/content"){
            Some(Value::Array(a))=> !a.iter().any(|item|item.get("type").and_then(Value::as_str)==Some("tool_result")),
            Some(Value::String(_))=>true,
            _=>false
        }
    }else if source=="codex" {
        t=="event_msg" && (event=="user_message" || event=="task_started")
    } else {false};
    if !start{return None;}
    Some(lookup(v,&["/turn_id","/payload/turn_id","/uuid"]).map(str::to_owned)
        .unwrap_or_else(||format!("at:{}:{}",digest(file.to_string_lossy().as_bytes())[..12].to_owned(),offset)))
}
fn session_id(source:&str,native:&str)->String{format!("s:{source}:{native}")}
fn turn_id(source:&str,sid:&str,native:&str)->String {
    format!("t:{source}:{}:{}", &digest(sid.as_bytes())[..16],native)
}
fn upsert_session(db:&Connection,source:&str,native:&str,v:&Value,event_id:&str,time:Option<i64>)->Result<(String,String,String,Option<String>)>{
    let sid=session_id(source,native);
    let existing=db.query_row("SELECT state,latest_at,worktree,evidence,parent_turn_id,project_id FROM sessions WHERE id=?1",
        [&sid],|r|Ok((r.get::<_,String>(0)?,r.get::<_,Option<i64>>(1)?,r.get::<_,String>(2)?,
        r.get::<_,String>(3)?,r.get::<_,Option<String>>(4)?,r.get::<_,String>(5)?))).optional()?;
    let cwd=lookup(v,&["/cwd","/payload/cwd","/payload/project/cwd","/payload/workspace/cwd","/workspace/cwd"])
       .map(str::to_owned).or_else(||existing.as_ref().map(|x|x.2.clone())).unwrap_or_default();
    let (pid,worktree,branch)=project(db,&cwd,&sid)?;
    let ev=lookup(v,&["/event","/payload/type"]).unwrap_or_default();
    let prior=existing.as_ref().map(|x|x.0.as_str()).unwrap_or("unknown");
    let previous_at=existing.as_ref().and_then(|x|x.1);
    let new_state=merge_state(prior,state(ev,v,time),previous_at,time);
    let parent_turn=lookup(v,&["/parent_turn_id","/payload/parent_turn_id"]).map(str::to_owned)
        .or_else(||existing.as_ref().and_then(|x|x.4.clone()));
    let proof=evidence(existing.as_ref().map(|x|x.3.clone()),event_id);
    db.execute("INSERT INTO sessions(id,project_id,source,native_id,parent_turn_id,started_at,ended_at,latest_at,state,worktree,branch,evidence)
       VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
       ON CONFLICT(id) DO UPDATE SET
       project_id=excluded.project_id,parent_turn_id=COALESCE(excluded.parent_turn_id,sessions.parent_turn_id),
       started_at=CASE WHEN sessions.started_at IS NULL OR excluded.started_at<sessions.started_at THEN excluded.started_at ELSE sessions.started_at END,
       ended_at=COALESCE(excluded.ended_at,sessions.ended_at),
       latest_at=CASE WHEN excluded.latest_at>COALESCE(sessions.latest_at,0) THEN excluded.latest_at ELSE sessions.latest_at END,
       state=excluded.state,worktree=COALESCE(NULLIF(excluded.worktree,''),sessions.worktree),
       branch=COALESCE(excluded.branch,sessions.branch),evidence=excluded.evidence",
       params![sid,pid,source,native,parent_turn,time,
         if new_state=="ended"{time}else{None},time,new_state,worktree,branch,proof])?;
    Ok((sid,pid,worktree,branch))
}
fn upsert_turn(db:&Connection,sid:&str,native_turn:&str,source:&str,pid:&str,worktree:&str,branch:Option<&str>,v:&Value,event_id:&str,time:Option<i64>)->Result<String>{
    let id=turn_id(source,sid,native_turn);
    let prev=db.query_row("SELECT state,latest_at,name,evidence FROM workflows WHERE id=?1",[&id],|r|
      Ok((r.get::<_,String>(0)?,r.get::<_,Option<i64>>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?))).optional()?;
    let explicit=lookup(v,&["/task_name","/workflow_name","/payload/task_name","/payload/workflow_name"]);
    let name=explicit.map(str::to_owned).or_else(||prev.as_ref().map(|x|x.2.clone()))
        .unwrap_or_else(||format!("Turn {}",native_turn.chars().take(12).collect::<String>()));
    let event=lookup(v,&["/event","/payload/type"]).unwrap_or("unknown");
    let new_state=merge_state(prev.as_ref().map(|x|x.0.as_str()).unwrap_or("unknown"),
      state(event,v,time),prev.as_ref().and_then(|x|x.1),time);
    let proof=evidence(prev.as_ref().map(|x|x.3.clone()),event_id);
    let parent=lookup(v,&["/parent_turn_id","/payload/parent_turn_id"]).map(str::to_owned);
    let model=lookup(v,&["/message/model","/model","/payload/model"]);
    db.execute("INSERT INTO workflows(id,project_id,source,session_id,turn_id,parent_turn_id,name,state,started_at,ended_at,latest_at,worktree,branch,model,evidence)
       VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)
       ON CONFLICT(id) DO UPDATE SET
       project_id=excluded.project_id,parent_turn_id=COALESCE(excluded.parent_turn_id,workflows.parent_turn_id),
       name=CASE WHEN excluded.name LIKE 'Turn %' THEN workflows.name ELSE excluded.name END,
       state=excluded.state,
       started_at=CASE WHEN workflows.started_at IS NULL OR excluded.started_at<workflows.started_at THEN excluded.started_at ELSE workflows.started_at END,
       ended_at=COALESCE(excluded.ended_at,workflows.ended_at),
       latest_at=CASE WHEN excluded.latest_at>COALESCE(workflows.latest_at,0) THEN excluded.latest_at ELSE workflows.latest_at END,
       worktree=COALESCE(NULLIF(excluded.worktree,''),workflows.worktree),
       branch=COALESCE(excluded.branch,workflows.branch),model=COALESCE(excluded.model,workflows.model),
       evidence=excluded.evidence",
       params![id,pid,source,sid,native_turn,parent,name,new_state,time,
       if RunState::from_db(&new_state).is_terminal(){time}else{None},
       time,worktree,branch,model,proof])?;
    Ok(id)
}
fn upsert_node(db:&Connection,wf:&str,id:&str,parent:Option<&str>,child_session:Option<&str>,role:&str,model:Option<&str>,
    run_state:RunState,time:Option<i64>,ev:&str,tool:Option<&str>)->Result<()>{
    let previous=db.query_row("SELECT state,latest_at,evidence FROM nodes WHERE id=?1",[id],|r|
        Ok((r.get::<_,String>(0)?,r.get::<_,Option<i64>>(1)?,r.get::<_,String>(2)?))).optional()?;
    let s=merge_state(previous.as_ref().map(|x|x.0.as_str()).unwrap_or("unknown"),
        run_state,previous.as_ref().and_then(|x|x.1),time);
    let proof=evidence(previous.map(|x|x.2),ev);
    db.execute("INSERT INTO nodes(id,workflow_id,parent_id,child_session_id,role,model,state,started_at,ended_at,latest_at,last_tool,evidence)
       VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
       ON CONFLICT(id) DO UPDATE SET
       parent_id=COALESCE(excluded.parent_id,nodes.parent_id),
       child_session_id=COALESCE(excluded.child_session_id,nodes.child_session_id),
       role=CASE WHEN nodes.role='agent' THEN excluded.role ELSE nodes.role END,
       model=COALESCE(excluded.model,nodes.model),state=excluded.state,
       started_at=CASE WHEN nodes.started_at IS NULL OR excluded.started_at<nodes.started_at THEN excluded.started_at ELSE nodes.started_at END,
       ended_at=COALESCE(excluded.ended_at,nodes.ended_at),
       latest_at=CASE WHEN excluded.latest_at>COALESCE(nodes.latest_at,0) THEN excluded.latest_at ELSE nodes.latest_at END,
       last_tool=COALESCE(excluded.last_tool,nodes.last_tool),evidence=excluded.evidence",
       params![id,wf,parent,child_session,role,model,s,time,
           if RunState::from_db(&s).is_terminal(){time}else{None},time,tool,proof])?;
    Ok(())
}
fn event(db:&Connection,id:&str,wf:Option<&str>,sid:&str,turn:Option<&str>,node:&str,
    time:Option<i64>,kind:&str,detail:&str,src:&str,call_id:Option<&str>)->Result<()> {
    db.execute("INSERT OR IGNORE INTO events(id,workflow_id,session_id,turn_id,node_id,at,kind,detail,evidence_source,call_id,ingested_at)
      VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
      params![id,wf,sid,turn,node,time,kind,detail,src,call_id,store::now()])?;
    Ok(())
}
fn call(db:&Connection,wf:&str,node:&str,id:&str,tool:&str,time:Option<i64>,done:bool,result:Option<&str>)->Result<bool>{
    let previous=db.query_row("SELECT name FROM calls WHERE workflow_id=?1 AND node_id=?2 AND call_id=?3",
       params![wf,node,id],|r|r.get::<_,String>(0)).optional()?;
    db.execute("INSERT INTO calls(workflow_id,node_id,call_id,name,started_at,finished_at,result)
       VALUES(?1,?2,?3,?4,?5,?6,?7)
       ON CONFLICT(workflow_id,node_id,call_id) DO UPDATE SET
       name=CASE WHEN calls.name='' THEN excluded.name ELSE calls.name END,
       started_at=COALESCE(calls.started_at,excluded.started_at),
       finished_at=COALESCE(excluded.finished_at,calls.finished_at),
       result=COALESCE(excluded.result,calls.result)",
       params![wf,node,id,tool,if done{None}else{time},if done{time}else{None},result])?;
    Ok(previous.is_none())
}
fn process(db:&Connection,file:&Path,source:&str,generation:i64,offset:u64,
    current_turn:&mut Option<String>,v:&Value)->Result<()> {
    let real_source=if source=="hook"{lookup(v,&["/source"]).unwrap_or("unknown")}else{source};
    let Some(native)=native_session(file,source,v) else{return Ok(())};
    if native=="unknown"{return Ok(());}
    let evidence_id=format!("e:{}", &digest(format!("{}:{generation}:{offset}",file.display()))[..32]);
    let time=at(v);
    let (sid,pid,worktree,branch)=upsert_session(db,real_source,&native,v,&evidence_id,time)?;
    let event_name=lookup(v,&["/event","/ev","/payload/type","/type"]).unwrap_or("unknown");
    let explicit_turn=lookup(v,&["/turn_id","/payload/turn_id"]).map(str::to_owned);
    let start=native_turn_start(v,source,offset,file);
    if let Some(turn)=start{*current_turn=Some(turn);}
    let turn=if source=="hook"{explicit_turn}else{explicit_turn.or_else(||current_turn.clone())};
    let root_node=format!("{sid}:main");
    let source_label=if source=="hook"{"hook"}else{source};
    let Some(native_turn)=turn else{
        event(db,&evidence_id,None,&sid,None,&root_node,time,event_name,event_name,source_label,None)?;
        return Ok(());
    };
    let wf=upsert_turn(db,&sid,&native_turn,real_source,&pid,&worktree,branch.as_deref(),v,&evidence_id,time)?;
    let root=format!("{wf}:main");
    let agent_from_file=file.file_stem().and_then(|s|s.to_str()).and_then(|s|s.strip_prefix("agent-"));
    let agent=lookup(v,&["/agent_id","/payload/agent_id"]).or(agent_from_file);
    let node=agent.map(|a|format!("{wf}:agent:{a}")).unwrap_or_else(||root.clone());
    let role=if agent.is_some(){lookup(v,&["/agent_type","/payload/agent_type"]).unwrap_or("agent")}else{"main"};
    let parent_agent=lookup(v,&["/parent_agent_id","/payload/parent_agent_id"]);
    let parent=parent_agent.map(|p|if p=="main"{root.clone()}else{format!("{wf}:agent:{p}")});
    let child_session=lookup(v,&["/child_session_id","/payload/child_session_id"])
        .map(|s|session_id(real_source,s));
    let model=lookup(v,&["/model","/message/model","/payload/model"]);
    let s=state(event_name,v,time);
    if node!=root {
        upsert_node(db,&wf,&root,None,None,"main",None,RunState::Unknown,None,&evidence_id,None)?;
    }
    upsert_node(db,&wf,&node,parent.as_deref(),child_session.as_deref(),role,model,s,time,&evidence_id,None)?;
    event(db,&evidence_id,Some(&wf),&sid,Some(&native_turn),&node,time,event_name,event_name,source_label,None)?;
    if let Some(parts)=v.pointer("/message/content").and_then(Value::as_array){
        for (index,part) in parts.iter().enumerate(){
            let kind=lookup(part,&["/type"]).unwrap_or_default();
            if kind=="tool_use"{
                let tool=lookup(part,&["/name"]).unwrap_or("tool");
                let call_id=lookup(part,&["/id"]).map(str::to_owned)
                    .unwrap_or_else(||format!("{evidence_id}:{index}"));
                let fresh=call(db,&wf,&node,&call_id,tool,time,false,None)?;
                if fresh {
                    event(db,&format!("{evidence_id}:tool:{index}"),Some(&wf),&sid,Some(&native_turn),
                      &node,time,"tool_start",tool,source_label,Some(&call_id))?;
                }
                upsert_node(db,&wf,&node,None,None,role,model,s,time,&evidence_id,Some(tool))?;
                if tool=="Skill"{
                    if let Some(skill)=lookup(part,&["/input/skill"]){
                        event(db,&format!("{evidence_id}:skill:{index}"),Some(&wf),&sid,Some(&native_turn),
                          &node,time,"skill_confirmed",skill,source_label,Some(&call_id))?;
                    }
                }
                if tool=="Read" {
                    if let Some(path)=lookup(part,&["/input/file_path"]){
                        if Path::new(path).file_name().and_then(|x|x.to_str())==Some("SKILL.md"){
                            if let Some(name)=Path::new(path).parent().and_then(Path::file_name).and_then(|x|x.to_str()){
                                event(db,&format!("{evidence_id}:candidate:{index}"),Some(&wf),&sid,Some(&native_turn),
                                  &node,time,"skill_candidate",name,source_label,Some(&call_id))?;
                            }
                        }
                    }
                }
                // The tool invocation does not establish the actual child agent ID.
            }else if kind=="tool_result" {
                if let Some(call_id)=lookup(part,&["/tool_use_id"]){
                    let result=if part.get("is_error").and_then(Value::as_bool)==Some(true){"failed"}else{"returned"};
                    let _=call(db,&wf,&node,call_id,"",time,true,Some(result))?;
                    event(db,&format!("{evidence_id}:result:{index}"),Some(&wf),&sid,Some(&native_turn),
                      &node,time,"tool_end",result,source_label,Some(call_id))?;
                }
            }
        }
    }
    if let Some(tool)=lookup(v,&["/tool_name","/tool","/payload/tool_name"]){
        let call_id=lookup(v,&["/tool_use_id","/tool_call_id","/payload/tool_use_id"]);
        let done=event_name.to_lowercase().contains("post")||event_name.ends_with("End");
        let fresh=if let Some(id)=call_id {call(db,&wf,&node,id,tool,time,done,None)?}else{true};
        if fresh||done{
            event(db,&format!("{evidence_id}:hook"),Some(&wf),&sid,Some(&native_turn),&node,
                time,if done{"tool_end"}else{"tool_start"},tool,"hook",call_id)?;
        }
        upsert_node(db,&wf,&node,None,None,role,model,s,time,&evidence_id,Some(tool))?;
    }
    if let Some(skill)=lookup(v,&["/skill_name"]){
        event(db,&format!("{evidence_id}:skill"),Some(&wf),&sid,Some(&native_turn),&node,
            time,"skill_confirmed",skill,"hook",None)?;
    }
    if let Some(p)=v.get("payload"){
        match lookup(p,&["/type"]).unwrap_or_default(){
            "function_call" => {
                let tool=lookup(p,&["/name"]).unwrap_or("tool");
                let call_id=lookup(p,&["/call_id"]).unwrap_or(&evidence_id);
                if call(db,&wf,&node,call_id,tool,time,false,None)?{
                    event(db,&format!("{evidence_id}:codex_tool"),Some(&wf),&sid,Some(&native_turn),
                        &node,time,"tool_start",tool,"codex",Some(call_id))?;
                }
                upsert_node(db,&wf,&node,None,None,role,model,s,time,&evidence_id,Some(tool))?;
            }
            "function_call_output" => {
                if let Some(call_id)=lookup(p,&["/call_id"]){
                    let _=call(db,&wf,&node,call_id,"",time,true,None)?;
                    event(db,&format!("{evidence_id}:codex_tool_end"),Some(&wf),&sid,Some(&native_turn),
                        &node,time,"tool_end","returned","codex",Some(call_id))?;
                }
            }
            "task_complete" => {
                // turn end != task success; use explicit ended state, never success.
                db.execute("UPDATE workflows SET state='ended',ended_at=COALESCE(ended_at,?2) WHERE id=?1 AND state NOT IN ('success','failed','cancelled')",
                   params![wf,time])?;
            }
            _=>{}
        }
    }
    // Subagent starts/stops only create the observed node. Parent links require explicit IDs.
    if event_name=="SubagentStart" || event_name=="SubagentStop" {
        if let Some(agent_id)=lookup(v,&["/agent_id","/payload/agent_id"]){
            let agent_node=format!("{wf}:agent:{agent_id}");
            let kind=lookup(v,&["/agent_type","/payload/agent_type"]).unwrap_or("agent");
            upsert_node(db,&wf,&agent_node,parent.as_deref(),child_session.as_deref(),kind,None,
                if event_name=="SubagentStop"{RunState::Ended}else{s},time,&evidence_id,None)?;
        }
    }
    db.execute("INSERT INTO changes(workflow_id,project_id,at) VALUES(?1,?2,?3)",
        params![wf,pid,store::now()])?;
    Ok(())
}
fn scan_file(db:&mut Connection,file:&Path,source:&str,report:&mut Report)->Result<()> {
    let path=file.to_string_lossy().into_owned();
    let identity=identity(file)?;
    let prefix=prefix(file)?;
    let size=fs::metadata(file)?.len();
    let previous:Option<(String,String,i64,i64,Option<String>)>=db.query_row(
        "SELECT identity,prefix,offset,generation,current_turn FROM files WHERE path=?1",[&path],
        |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?;
    let mut generation=previous.as_ref().map(|p|p.3).unwrap_or(0);
    let mut offset=previous.as_ref().map(|p|p.2 as u64).unwrap_or(0);
    let mut turn=previous.as_ref().and_then(|p|p.4.clone());
    if let Some((old_id,old_prefix,_,_,_))=previous.as_ref(){
        if old_id!=&identity || old_prefix!=&prefix || size<offset {
            generation+=1;offset=0;turn=None;
            store::issue(db,&path,"Source replaced or truncated; prior history retained, new generation started")?;
            report.issues+=1;
        }
    }
    if size==offset {return Ok(());}
    let mut reader=BufReader::new(File::open(file)?);
    reader.seek(SeekFrom::Start(offset))?;
    let maximum=(size-offset).min(4*1024*1024);
    let tx=db.transaction()?;
    let mut end=offset;
    let mut invalid=0;
    let mut buffer=Vec::new();
    while end-offset<maximum {
        buffer.clear();
        let start=end;
        let n=reader.read_until(b'\n',&mut buffer)?;
        if n==0{break;}
        if !buffer.ends_with(b"\n"){break;}
        end+=n as u64;
        match serde_json::from_slice::<Value>(&buffer){
            Ok(value)=>{process(&tx,file,source,generation,start,&mut turn,&value)?;report.events+=1;}
            Err(_)=>invalid+=1
        }
    }
    tx.execute("INSERT INTO files(path,identity,prefix,offset,generation,current_turn,incomplete)
       VALUES(?1,?2,?3,?4,?5,?6,?7)
       ON CONFLICT(path) DO UPDATE SET identity=excluded.identity,prefix=excluded.prefix,
       offset=excluded.offset,generation=excluded.generation,current_turn=excluded.current_turn,
       incomplete=CASE WHEN files.incomplete=1 OR excluded.incomplete=1 THEN 1 ELSE 0 END",
       params![path,identity,prefix,end as i64,generation,turn,if invalid>0{1}else{0}])?;
    if invalid>0{
        store::issue(&tx,&path,&format!("{invalid} corrupted JSONL lines skipped"))?;
        report.issues+=1;
    }
    tx.commit()?;
    report.files+=1;
    Ok(())
}
pub fn refresh(db:&mut Connection,sources:&Sources)->Result<Report>{
    let mut report=Report::default();
    for (source,root) in [("claude",&sources.claude),("codex",&sources.codex)]{
        if !root.exists(){continue;}
        for entry in WalkDir::new(root).max_depth(15){
            let entry=match entry {
                Ok(e)=>e,Err(e)=>{store::issue(db,"walkdir",&e.to_string())?;report.issues+=1;continue;}
            };
            if !entry.file_type().is_file()||entry.path().extension().and_then(|s|s.to_str())!=Some("jsonl"){continue;}
            if let Err(e)=scan_file(db,entry.path(),source,&mut report) {
                store::issue(db,&entry.path().to_string_lossy(),&e.to_string())?;
                report.issues+=1;
            }
        }
    }
    if sources.hooks.exists(){
        if let Err(e)=scan_file(db,&sources.hooks,"hook",&mut report){
            store::issue(db,&sources.hooks.to_string_lossy(),&e.to_string())?;
            report.issues+=1;
        }
    }
    let threshold=store::now()-120;
    db.execute("UPDATE workflows SET state='inactive' WHERE state='recent' AND latest_at<?1",[threshold])?;
    db.execute("UPDATE sessions SET state='inactive' WHERE state='recent' AND latest_at<?1",[threshold])?;
    db.execute("UPDATE nodes SET state='inactive' WHERE state='recent' AND latest_at<?1",[threshold])?;
    db.execute("DELETE FROM projects WHERE NOT EXISTS(SELECT 1 FROM sessions WHERE sessions.project_id=projects.id)
         AND NOT EXISTS(SELECT 1 FROM workflows WHERE workflows.project_id=projects.id)",[])?;
    Ok(report)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn scopes_native_ids(){
        assert_ne!(session_id("codex","abc"),session_id("claude","abc"));
        assert_ne!(turn_id("codex","abc","1"),turn_id("claude","abc","1"));
    }
    #[test] fn end_never_success(){
        assert_eq!(state("SessionEnd",&serde_json::json!({}),Some(store::now())),RunState::Ended);
        assert_eq!(state("tool_use",&serde_json::json!({}),Some(store::now())),RunState::Recent);
    }
    #[test] fn different_repos_not_merged(){
        let d=tempfile::tempdir().unwrap();
        fs::create_dir_all(d.path().join("a")).unwrap();
        fs::create_dir_all(d.path().join("b")).unwrap();
        assert_ne!(project_location(d.path().join("a").to_str().unwrap()).0,project_location(d.path().join("b").to_str().unwrap()).0);
    }
}
