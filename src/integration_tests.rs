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
