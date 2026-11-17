use crate::{collector::{self,Sources},store};
use std::{fs,io::Write,path::Path};
use tempfile::TempDir;

fn write(path:&Path,fixture:&str,alpha:&Path,beta:&Path){
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path,fixture.replace("__ALPHA__",alpha.to_str().unwrap())
        .replace("__BETA__",beta.to_str().unwrap())).unwrap();
}
struct Setup {
    dir:TempDir,
    sources:Sources,
    alpha:String,
    beta:String,
}
impl Setup {
    fn new()->Self{
        let dir=tempfile::tempdir().unwrap();
        let a=dir.path().join("alpha");
        let b=dir.path().join("beta");
        fs::create_dir_all(a.join(".git")).unwrap();
        fs::create_dir_all(b.join(".git")).unwrap();
        let sources=Sources{
            claude:dir.path().join("claude"),
            codex:dir.path().join("codex"),
            hooks:dir.path().join("hook-events.jsonl"),
        };
        let folder=sources.claude.join("project-a");
        write(&folder.join("sess-a.jsonl"),include_str!("../tests/fixtures/claude_a.jsonl"),&a,&b);
        write(&folder.join("sess-b.jsonl"),include_str!("../tests/fixtures/claude_b.jsonl"),&a,&b);
        write(&folder.join("child-1.jsonl"),include_str!("../tests/fixtures/claude_child1.jsonl"),&a,&b);
        write(&sources.codex.join("2026/11/17/rollout-2026-11-16-550e8400-e29b-41d4-a716-446655440000.jsonl"),
            include_str!("../tests/fixtures/codex.jsonl"),&a,&b);
        write(&sources.hooks,include_str!("../tests/fixtures/hooks.jsonl"),&a,&b);
        Self{dir,sources,alpha:a.to_string_lossy().into_owned(),beta:b.to_string_lossy().into_owned()}
    }
    fn db(&self)->rusqlite::Connection{
        store::open_at(&self.dir.path().join("history.sqlite")).unwrap()
    }
}
fn id(db:&rusqlite::Connection,native:&str)->String{
    db.query_row("SELECT id FROM workflows WHERE turn_id=?1",[native],|r|r.get(0)).unwrap()
}
#[test]
fn scoped_execution_ids_status_and_nested_delegation_survive_restart(){
    let setup=Setup::new();
    let mut db=setup.db();
    let report=collector::refresh(&mut db,&setup.sources).unwrap();
    assert_eq!(report.issues,0);
    let projects=store::projects(&db,None,None,0,20).unwrap().items;
    assert_eq!(projects.len(),2, "two distinct repositories");
    let alpha=projects.iter().find(|p|p.root==setup.alpha).unwrap();
    let beta=projects.iter().find(|p|p.root==setup.beta).unwrap();
    assert_ne!(alpha.id,beta.id);
    let sessions=store::sessions(&db,Some(&alpha.id),None,0,20).unwrap().items;
    assert!(sessions.iter().any(|s|s.native_id=="sess-a"));
    assert!(sessions.iter().any(|s|s.native_id=="sess-b"));
    assert!(sessions.iter().any(|s|s.native_id=="child-1"));
    assert!(sessions.iter().any(|s|s.native_id=="child-2"));
    let a="s:claude:sess-a";
    let turns=store::workflows(&db,None,Some(a),None,None,None,0,20).unwrap().items;
    assert_eq!(turns.len(),2, "two independent turns in a single session");
    let id1=id(&db,"turn-a1");
    let id2=id(&db,"turn-a2");
    assert_ne!(id1,id2);
    let all=store::workflows(&db,None,None,None,None,None,0,100).unwrap().items;
    let alpha_runs=store::workflows(&db,Some(&alpha.id),None,None,None,None,0,20).unwrap().items;
    assert!(all.iter().any(|w|w.id==id1));
    assert!(alpha_runs.iter().any(|w|w.id==id1));
    assert!(turns.iter().any(|w|w.id==id1));
    let by_turn=store::workflows(&db,None,Some(a),Some("turn-a1"),None,None,0,20).unwrap().items;
    assert_eq!(by_turn.len(),1);
    assert_eq!(by_turn[0].id,id1);

    let detailed=store::workflow(&db,&id1).unwrap().unwrap();
    let ag1=detailed.nodes.iter().find(|n|n.role=="reviewer").unwrap();
    let ag2=detailed.nodes.iter().find(|n|n.role=="explorer").unwrap();
    assert_eq!(ag1.parent_id.as_deref(),Some(format!("{id1}:main").as_str()));
    assert_eq!(ag2.parent_id.as_deref(),Some(ag1.id.as_str()));
    assert_eq!(ag1.child_session_id.as_deref(),Some("s:claude:child-1"));
    assert_eq!(detailed.edges.len(),2);
    assert_eq!(ag1.state.as_str(),"ended");
    let child=sessions.iter().find(|s|s.native_id=="child-1").unwrap();
    assert_eq!(child.parent_turn_id.as_deref(),Some(id1.as_str()));
    let child2=sessions.iter().find(|s|s.native_id=="child-2").unwrap();
    assert_eq!(child2.parent_turn_id.as_deref(),Some(id(&db,"turn-child-1").as_str()));
    assert_eq!(store::workflow(&db,&id2).unwrap().unwrap().workflow.state.as_str(),"success");
    assert_eq!(store::workflow(&db,&id(&db,"turn-b1")).unwrap().unwrap().workflow.state.as_str(),"failed");
    assert_eq!(store::workflow(&db,&id(&db,"turn-c1")).unwrap().unwrap().workflow.state.as_str(),"cancelled");
    assert_eq!(store::sessions(&db,Some(&beta.id),None,0,20).unwrap().items.len(),1);

    let count_before:i64=db.query_row("SELECT COUNT(*) FROM events",[],|r|r.get(0)).unwrap();
    let calls_before:i64=db.query_row("SELECT COUNT(*) FROM calls",[],|r|r.get(0)).unwrap();
    collector::refresh(&mut db,&setup.sources).unwrap();
    assert_eq!(count_before,db.query_row("SELECT COUNT(*) FROM events",[],|r|r.get::<_,i64>(0)).unwrap());
    assert_eq!(calls_before,db.query_row("SELECT COUNT(*) FROM calls",[],|r|r.get::<_,i64>(0)).unwrap());
    drop(db);
    let db=setup.db();
    assert_eq!(store::workflow(&db,&id1).unwrap().unwrap().edges.len(),2);
    let dump=fs::read(setup.dir.path().join("history.sqlite")).unwrap();
    for secret in ["DO_NOT_STORE_PROMPT_A","DO_NOT_STORE_DESCRIPTION","DO_NOT_STORE_TOOL_ARGUMENT",
                   "DO_NOT_STORE_CODEX_OUTPUT","DO_NOT_STORE_COMMAND"]{
        assert!(!dump.windows(secret.len()).any(|bytes|bytes==secret.as_bytes()),"raw content leaked: {secret}");
    }
}
#[test]
fn replacement_and_invalid_json_keep_history_and_report_incompleteness(){
    let setup=Setup::new();
    let mut db=setup.db();
    collector::refresh(&mut db,&setup.sources).unwrap();
    let id1=id(&db,"turn-a1");
    let path=setup.sources.claude.join("project-a/sess-a.jsonl");
    let text=fs::read_to_string(&path).unwrap();
    fs::write(&path,text.lines().next().unwrap().to_string()+"\n").unwrap();
    let report=collector::refresh(&mut db,&setup.sources).unwrap();
    assert!(report.issues>0);
    assert!(store::workflow(&db,&id1).unwrap().unwrap().workflow.incomplete);
    let count:i64=db.query_row("SELECT COUNT(*) FROM workflows WHERE session_id='s:claude:sess-a'",[],|r|r.get(0)).unwrap();
    assert_eq!(count,2, "old turn history remains accessible");
    let mut file=fs::OpenOptions::new().append(true).open(&path).unwrap();
    writeln!(file,"{{this is broken json").unwrap();
    let report=collector::refresh(&mut db,&setup.sources).unwrap();
    assert!(report.issues>0);
    assert!(!store::projects(&db,None,None,0,20).unwrap().issues.is_empty());
}
#[test]
fn worktree_shares_project_identity_but_retains_worktree_path(){
    let setup=Setup::new();
    let main=setup.dir.path().join("shared");
    let wt=setup.dir.path().join("feature-wt");
    fs::create_dir_all(main.join(".git/worktrees/wt1")).unwrap();
    fs::create_dir_all(&wt).unwrap();
    fs::write(wt.join(".git"),format!("gitdir: {}\n",main.join(".git/worktrees/wt1").display())).unwrap();
    fs::write(main.join(".git/worktrees/wt1/commondir"),"../..\n").unwrap();
    fs::write(main.join(".git/worktrees/wt1/HEAD"),"ref: refs/heads/feature\n").unwrap();
    // Worktree identification is exercised through the public collector.
    let path=setup.sources.claude.join("project-b/feature.jsonl");
    write(&path,"{\"type\":\"user\",\"uuid\":\"wt-turn\",\"cwd\":\"__ALPHA__\",\"timestamp\":\"2026-11-16T15:30:00Z\",\"message\":{\"content\":\"ok\"}}\n",
        &wt,Path::new(&setup.beta));
    let mut db=setup.db();
    collector::refresh(&mut db,&setup.sources).unwrap();
    let turn=id(&db,"wt-turn");
    let wf=store::workflow(&db,&turn).unwrap().unwrap().workflow;
    assert_eq!(wf.worktree,wt.to_string_lossy());
    assert_eq!(wf.branch.as_deref(),Some("feature"));
    let root:String=db.query_row("SELECT root FROM projects WHERE id=?1",[wf.project_id],|r|r.get(0)).unwrap();
    assert_eq!(root,main.to_string_lossy());
}

#[test]
fn http_api_filters_before_delivery_and_preserves_execution_identity() {
    use axum::{body::{Body,to_bytes},http::{Request,StatusCode}};
    use tower::ServiceExt;
    let setup=Setup::new();
    let mut db=setup.db();
    collector::refresh(&mut db,&setup.sources).unwrap();
    let all=store::projects(&db,None,None,0,20).unwrap().items;
    let pid=all.iter().find(|p|p.root==setup.alpha).unwrap().id.clone();
    let target=id(&db,"turn-a1");
    drop(db);
    let assets=setup.dir.path().join("assets");
    fs::create_dir_all(&assets).unwrap();
    fs::write(assets.join("index.html"),"ok").unwrap();
    let app=crate::server::app(setup.dir.path().join("history.sqlite"),&assets);
    let runtime=tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let get=|uri:&str| {
        let app=app.clone();
        runtime.block_on(async {
            let response=app.oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap()).await.unwrap();
            let status=response.status();
            let body=to_bytes(response.into_body(),4*1024*1024).await.unwrap();
            (status,serde_json::from_slice::<serde_json::Value>(&body).unwrap())
        })
    };
    let (status,projects)=get("/api/v1/projects?limit=20");
    assert_eq!(status,StatusCode::OK);
    assert_eq!(projects["items"].as_array().unwrap().len(),2);
    let (status,sessions)=get(&format!("/api/v1/sessions?project_id={pid}"));
    assert_eq!(status,StatusCode::OK);
    assert!(sessions["items"].as_array().unwrap().len()>=2);
    let (status,turns)=get("/api/v1/workflows?session_id=s:claude:sess-a");
    assert_eq!(status,StatusCode::OK);
    assert_eq!(turns["items"].as_array().unwrap().len(),2);
    let (status,turn)=get("/api/v1/workflows?session_id=s:claude:sess-a&turn_id=turn-a1");
    assert_eq!(status,StatusCode::OK);
    let items=turn["items"].as_array().unwrap();
    assert_eq!(items.len(),1);
    assert_eq!(items[0]["id"],target);
    let (status,detail)=get(&format!("/api/v1/workflows/{target}"));
    assert_eq!(status,StatusCode::OK);
    assert_eq!(detail["workflow"]["id"],items[0]["id"]);
    assert_eq!(detail["edges"].as_array().unwrap().len(),2);
    let (status,events)=get("/api/v1/sessions/s:claude:sess-a/events?cursor=0");
    assert_eq!(status,StatusCode::OK);
    assert!(events["items"].as_array().unwrap().len()>2);
    let (status,_)=get("/api/v1/changes?after=invalid");
    assert_eq!(status,StatusCode::BAD_REQUEST);
}

#[test]
fn codex_task_started_and_user_message_do_not_fabricate_two_turns() {
    let d=tempfile::tempdir().unwrap();
    let codex=d.path().join("codex/rollout-2026-11-17-550e8400-e29b-41d4-a716-446655440000.jsonl");
    fs::create_dir_all(codex.parent().unwrap()).unwrap();
    let records=[
        serde_json::json!({"type":"event_msg","timestamp":"2026-11-17T01:00:00Z","payload":{"type":"task_started"}}),
        serde_json::json!({"type":"event_msg","timestamp":"2026-11-17T01:00:01Z","payload":{"type":"user_message","message":"DO_NOT_PERSIST"}}),
        serde_json::json!({"type":"response_item","timestamp":"2026-11-17T01:00:02Z","payload":{"type":"function_call","name":"exec_command","call_id":"call-one"}}),
        serde_json::json!({"type":"event_msg","timestamp":"2026-11-17T01:00:03Z","payload":{"type":"task_complete"}}),
        serde_json::json!({"type":"event_msg","timestamp":"2026-11-17T01:01:00Z","payload":{"type":"task_started"}}),
        serde_json::json!({"type":"event_msg","timestamp":"2026-11-17T01:01:01Z","payload":{"type":"user_message","message":"DO_NOT_PERSIST_TWO"}}),
    ];
    fs::write(&codex,records.iter().map(|v|v.to_string()).collect::<Vec<_>>().join("\n")+"\n").unwrap();
    let sources=Sources{claude:d.path().join("claude"),codex:d.path().join("codex"),hooks:d.path().join("none")};
    let mut db=store::open_at(&d.path().join("history.sqlite")).unwrap();
    collector::refresh(&mut db,&sources).unwrap();
    let session="s:codex:550e8400-e29b-41d4-a716-446655440000";
    let turns=store::workflows(&db,None,Some(session),None,None,None,0,10).unwrap();
    assert_eq!(turns.items.len(),2,"each task must have exactly one turn");
}
#[test]
fn short_file_growth_does_not_flag_a_false_rotation() {
    let d=tempfile::tempdir().unwrap();
    let file=d.path().join("claude/example.jsonl");
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file,"{}\n").unwrap();
    let sources=Sources{claude:d.path().join("claude"),codex:d.path().join("codex"),hooks:d.path().join("none")};
    let mut db=store::open_at(&d.path().join("history.sqlite")).unwrap();
    collector::refresh(&mut db,&sources).unwrap();
    fs::OpenOptions::new().append(true).open(&file).unwrap()
        .write_all(b"{\"type\":\"user\",\"uuid\":\"new-turn\",\"timestamp\":\"2026-11-17T00:00:00Z\",\"message\":{\"content\":\"a\"}}\n").unwrap();
    let result=collector::refresh(&mut db,&sources).unwrap();
    assert_eq!(result.issues,0,"file append must not be flagged as rotation");
}

#[test]
fn expiration_publishes_changes_for_sessions_and_agents_without_new_source_lines() {
    let setup=Setup::new();
    let mut db=setup.db();
    collector::refresh(&mut db,&setup.sources).unwrap();
    let wf=id(&db,"turn-a1");
    let before=store::last_seq(&db).unwrap();
    let session="s:claude:sess-a";
    // Simulate old activity without modifying any original transcript.
    db.execute("UPDATE sessions SET state='recent',latest_at=1 WHERE id=?1",[session]).unwrap();
    db.execute("UPDATE nodes SET state='recent',latest_at=1 WHERE workflow_id=?1",[&wf]).unwrap();
    db.execute("UPDATE workflows SET state='recent',latest_at=1 WHERE id=?1",[&wf]).unwrap();
    collector::refresh(&mut db,&setup.sources).unwrap();
    let session_state:String=db.query_row("SELECT state FROM sessions WHERE id=?1",[session],|r|r.get(0)).unwrap();
    assert_eq!(session_state,"inactive");
    let node_count:i64=db.query_row("SELECT COUNT(*) FROM nodes WHERE workflow_id=?1 AND state='inactive'",
        [&wf],|r|r.get(0)).unwrap();
    assert!(node_count>0);
    let updates=store::changes(&db,before,100).unwrap();
    assert!(updates.items.iter().any(|c|c.workflow_id==wf));
    assert!(updates.items.iter().any(|c|c.workflow_id.is_empty()));
    let after=store::last_seq(&db).unwrap();
    collector::refresh(&mut db,&setup.sources).unwrap();
    assert_eq!(store::last_seq(&db).unwrap(),after,"expiry must not create repeated notifications");
}

#[test]
fn project_pagination_and_status_filter_are_applied_in_sqlite() {
    let d=tempfile::tempdir().unwrap();
    let db=store::open_at(&d.path().join("paged.sqlite")).unwrap();
    for n in 0..120 {
        let pid=format!("project-{n:03}");
        db.execute("INSERT INTO projects(id,name,root) VALUES(?1,?2,?3)",
            rusqlite::params![pid,pid,format!("/repo/{pid}")]).unwrap();
        let wf=format!("wf-{n}");
        let state=if n%2==0{"recent"}else{"failed"};
        db.execute("INSERT INTO workflows(id,project_id,source,session_id,turn_id,name,state,latest_at)
            VALUES(?1,?2,'claude',?3,?4,?5,?6,?7)",
            rusqlite::params![wf,pid,format!("s-{n}"),format!("turn-{n}"),
                format!("task {n}"),state,n as i64]).unwrap();
    }
    let first=store::projects(&db,None,None,0,40).unwrap();
    let second=store::projects(&db,None,None,40,40).unwrap();
    let third=store::projects(&db,None,None,80,40).unwrap();
    assert_eq!(first.items.len(),40);
    assert_eq!(second.items.len(),40);
    assert_eq!(third.items.len(),40);
    assert_eq!(first.next_offset,Some(40));
    assert_eq!(third.next_offset,None);
    let mut ids=std::collections::HashSet::new();
    for p in first.items.iter().chain(second.items.iter()).chain(third.items.iter()) {
        assert!(ids.insert(&p.id),"duplicated project between pages");
    }
    let attention=store::projects(&db,Some("attention"),None,0,100).unwrap();
    let completed=store::projects(&db,Some("completed"),None,0,100).unwrap();
    assert_eq!(attention.items.len(),60);
    assert_eq!(completed.items.len(),60);
    assert!(attention.items.iter().all(|p|p.attention_count==1 && p.ended_count==1));
    assert!(completed.items.iter().all(|p|p.ended_count==1));
    assert_eq!(store::projects(&db,None,Some("task 11"),0,100).unwrap().items.len(),11);
}

#[test]
fn new_collection_issue_notifies_once_without_repeating_on_poll() {
    let d=tempfile::tempdir().unwrap();
    let db=store::open_at(&d.path().join("issues.sqlite")).unwrap();
    assert_eq!(store::last_seq(&db).unwrap(),0);
    store::issue(&db,"/invalid/file","parse error").unwrap();
    let after=store::last_seq(&db).unwrap();
    assert_eq!(after,1);
    store::issue(&db,"/invalid/file","parse error").unwrap();
    assert_eq!(store::last_seq(&db).unwrap(),after);
    store::issue(&db,"/invalid/file","read error").unwrap();
    assert_eq!(store::last_seq(&db).unwrap(),after+1);
    let page=store::changes(&db,0,10).unwrap();
    assert!(page.items.iter().all(|c|c.project_id.is_empty() && c.workflow_id.is_empty()));
}
