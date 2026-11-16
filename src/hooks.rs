use crate::scan;
use anyhow::{Context, Result};
use serde_json::{json,Value};
use std::fs::{self,OpenOptions};
use std::io::{self,Read,Write};
use std::path::{Path,PathBuf};

const MARKER:&str="treills-metadata-v1";
const EVENTS:&[&str]=&["SessionStart","SessionEnd","PreToolUse","PostToolUse","SubagentStart","SubagentStop","Stop"];
fn home()->PathBuf{dirs::home_dir().unwrap_or_else(||PathBuf::from("."))}
fn backup(path:&Path)->Result<()> {
    if path.exists(){
        let b=path.with_extension(format!("treills-backup-{}.json",std::process::id()));
        fs::copy(path,&b)?;
        eprintln!("backup: {}",b.display());
    }
    Ok(())
}
fn load(path:&Path)->Result<Value>{
    if path.exists() {Ok(serde_json::from_slice(&fs::read(path)?)?)} else {Ok(json!({}))}
}
fn save(path:&Path,value:&Value)->Result<()>{
    if let Some(parent)=path.parent(){fs::create_dir_all(parent)?;}
    backup(path)?;
    let tmp=path.with_extension(format!("treills-{}.tmp",std::process::id()));
    fs::write(&tmp,format!("{}\n",serde_json::to_string_pretty(value)?))?;
    fs::rename(tmp,path)?;
    Ok(())
}
fn hook_command(event:&str,source:&str)->Result<String>{
    let exe=std::env::current_exe()?;
    let path=exe.to_str().context("binary path is not UTF-8")?;
    // Paths in POSIX single quotes: internal single quotes replaced with '\''
    let quoted=format!("'{}'",path.replace('\'',"'\\''"));
    Ok(format!("TREILLS_SOURCE={source} {quoted} hook {event}"))
}
fn hooks_add(root:&mut Value,source:&str)->Result<usize>{
    if !root.is_object(){anyhow::bail!("settings root must be an object");}
    if root.get("hooks").is_none(){root["hooks"]=json!({});}
    let mut added=0;
    for event in EVENTS {
        if root["hooks"].get(event).is_none(){root["hooks"][event]=json!([]);}
        let groups=root["hooks"][event].as_array_mut().context("unexpected hooks schema")?;
        let exists=groups.iter().any(|g|g.get("description").and_then(Value::as_str)==Some(MARKER));
        if exists{continue;}
        groups.push(json!({"description":MARKER,"hooks":[{"type":"command","command":hook_command(event,source)?,"timeout":5}]}));
        added+=1;
    }
    Ok(added)
}
fn hooks_remove(root:&mut Value)->usize {
    let mut removed=0;
    if let Some(hooks)=root.get_mut("hooks").and_then(Value::as_object_mut){
        for groups in hooks.values_mut(){
            if let Some(arr)=groups.as_array_mut(){
                let before=arr.len();
                arr.retain(|g|g.get("description").and_then(Value::as_str)!=Some(MARKER));
                removed+=before-arr.len();
            }
        }
    }
    removed
}
pub fn install(with_codex:bool,dry:bool)->Result<()> {
    let mut paths=vec![home().join(".claude/settings.json")];
    if with_codex {paths.push(home().join(".codex/hooks.json"));}
    for path in paths{
        let mut root=load(&path)?;
        let source=if path.to_string_lossy().contains(".codex/"){"codex"}else{"claude"};
        let n=hooks_add(&mut root,source)?;
        if dry {println!("would add {n} hook groups in {}",path.display());}
        else if n>0 {save(&path,&root)?;println!("installed {n} hook groups in {}",path.display());}
        else {println!("already installed: {}",path.display());}
    }
    Ok(())
}
pub fn uninstall()->Result<()> {
    for path in [home().join(".claude/settings.json"),home().join(".codex/hooks.json")]{
        if !path.exists(){continue;}
        let mut root=load(&path)?;
        let n=hooks_remove(&mut root);
        if n>0{save(&path,&root)?;}
        println!("removed {n} hook groups from {}",path.display());
    }
    Ok(())
}
fn text(v:&Value,key:&str)->Option<String>{
    v.get(key).or_else(||v.get("payload").and_then(|p|p.get(key)))
        .and_then(Value::as_str).map(ToOwned::to_owned)
}
pub fn emit(event:&str)->Result<()> {
    if !EVENTS.contains(&event){anyhow::bail!("invalid hook event");}
    let mut input=String::new();
    io::stdin().take(1024*1024).read_to_string(&mut input)?;
    let v:Value=serde_json::from_str(&input).unwrap_or_else(|_|json!({}));
    let mut row=json!({
        "source": std::env::var("TREILLS_SOURCE").unwrap_or_else(|_|"claude".into()),
        "event":event,
        "ts_unix":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs(),
        "session_id":text(&v,"session_id").unwrap_or_else(||std::env::var("HARNESS_SESSION_ID").unwrap_or_else(|_|"unknown".into())),
    });
    for key in ["cwd","tool_name","tool_use_id","tool_call_id","agent_id","agent_type",
        "parent_agent_id","child_session_id","parent_session_id","parent_turn_id","turn_id",
        "skill_name","task_name","workflow_name","model","event_id","timestamp"] {
        if let Some(value)=text(&v,key){row[key]=json!(value);}
    }
    // Never persist prompts, tool arguments, tool outputs, or transcript content.
    let path=scan::event_file();
    if let Some(parent)=path.parent(){
        fs::create_dir_all(parent)?;
        #[cfg(unix)] {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(parent,fs::Permissions::from_mode(0o700))?;
        }
    }
    let mut f=OpenOptions::new().create(true).append(true).open(&path)?;
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path,fs::Permissions::from_mode(0o600))?;
    }
    writeln!(f,"{}",serde_json::to_string(&row)?)?;
    Ok(())
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn nested_hook_metadata_is_read_without_body_fields(){
        let v=json!({"payload":{"session_id":"session-1","turn_id":"turn-1",
          "tool_name":"Read","cwd":"/repo/project","tool_input":{"secret":"DO_NOT_STORE"}}});
        assert_eq!(text(&v,"session_id").as_deref(),Some("session-1"));
        assert_eq!(text(&v,"turn_id").as_deref(),Some("turn-1"));
        assert!(text(&v,"tool_input").is_none());
    }
    #[test]
    fn hook_merge_idempotent_and_removable(){
        let mut d=json!({"hooks":{"Stop":[{"hooks":[{"type":"command","command":"echo keep"}]}]}});
        let n=hooks_add(&mut d,"claude").unwrap();
        assert_eq!(n,EVENTS.len());
        assert_eq!(hooks_add(&mut d,"claude").unwrap(),0);
        assert_eq!(hooks_remove(&mut d),n);
        assert_eq!(d["hooks"]["Stop"].as_array().unwrap().len(),1);
    }
}
