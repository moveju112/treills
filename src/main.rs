mod collector;
mod domain;
mod server;
mod store;
mod hooks;
mod model;
mod scan;
mod tui;
use anyhow::{bail,Result};
use std::process::Command;

fn usage(){
    println!("treills 0.1.0 -- SSH-friendly Claude Code + Codex activity dashboard");
    println!("  monitor             show read-only dashboard (default)");
    println!("  serve [--bind IP] [--port N]    run loopback-only web dashboard");
    println!("  typegen             generate web/src/contracts.generated.ts");
    println!("  sessions            list detected sessions");
    println!("  events [--follow]   print metadata hook log");
    println!("  install [--dry-run] [--with-codex]   add metadata-only hooks");
    println!("  uninstall           remove only Treills hook groups");
    println!("  doctor              display paths and required tools");
    println!("  run --cli claude|codex   run CLI + monitor in tmux");
}
fn doctor(){
    println!("Treills {}",env!("CARGO_PKG_VERSION"));
    println!("event log: {}",scan::event_file().display());
    println!("sessions discovered: {}",scan::scan().len());
    for bin in ["claude","codex","tmux"] {
        let ok=Command::new("sh").arg("-c").arg(format!("command -v {bin} >/dev/null")).status()
            .map(|s|s.success()).unwrap_or(false);
        println!("{bin}: {}",if ok{"found"}else{"missing"});
    }
    println!("No changes are made by doctor.");
}
fn run_tmux(cli:&str)->Result<()>{
    if cli!="claude" && cli!="codex"{bail!("expected --cli claude or codex");}
    let exe=std::env::current_exe()?;
    let name=format!("treills-{}",std::process::id());
    let command=format!("'{}' monitor",exe.to_string_lossy().replace('\'',"'\\''"));
    let status=Command::new("tmux")
        .args(["new-session","-d","-s",&name,cli])
        .status()?;
    if !status.success(){bail!("tmux new-session failed");}
    let status=Command::new("tmux").args(["split-window","-h","-t",&name,&command]).status()?;
    if !status.success(){bail!("tmux split-window failed");}
    let _=Command::new("tmux").args(["select-pane","-t",&format!("{name}:0.0")]).status();
    let status=Command::new("tmux").args(["attach-session","-t",&name]).status()?;
    if !status.success(){bail!("tmux attach failed");}
    Ok(())
}
fn main()->Result<()>{
    let args:Vec<String>=std::env::args().skip(1).collect();
    let sub=args.first().map(String::as_str).unwrap_or("monitor");
    match sub {
        "monitor"=>tui::monitor(),
        "serve"=>{
            let flag=|name:&str,default:&str|->String{args.iter().position(|a|a==name).and_then(|i|args.get(i+1)).cloned().unwrap_or_else(||default.into())};
            let bind=flag("--bind","127.0.0.1");
            let port=flag("--port","47821").parse::<u16>()?;
            let assets=args.iter().position(|a|a=="--assets").and_then(|i|args.get(i+1)).map(String::as_str);
            server::serve(&bind,port,assets)
        },
        "typegen"=>{
            let target=std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("web/src/contracts.generated.ts");
            if let Some(parent)=target.parent(){std::fs::create_dir_all(parent)?;}
            std::fs::write(&target,domain::typescript())?;
            println!("generated {}",target.display());
            Ok(())
        },
        "sessions"=>{
            for s in scan::scan(){println!("{}\t{}\t{}\t{}\t{}",s.source,s.id,s.model,s.phase,s.cwd);}
            Ok(())
        }
        "events"=>scan::read_events(args.iter().any(|s|s=="--follow"||s=="-f")),
        "install"=>hooks::install(args.iter().any(|s|s=="--with-codex"),args.iter().any(|s|s=="--dry-run")),
        "uninstall"=>hooks::uninstall(),
        "doctor"=>{doctor();Ok(())},
        "run"=> {
            let index=args.iter().position(|s|s=="--cli");
            let cli=index.and_then(|i|args.get(i+1)).map(String::as_str).unwrap_or("claude");
            run_tmux(cli)
        }
        "hook"=> {
            let name=args.get(1).map(String::as_str).unwrap_or("");
            hooks::emit(name)
        }
        "-h"|"--help"|"help"=>{usage();Ok(())},
        "-V"|"--version"=>{println!("{}",env!("CARGO_PKG_VERSION"));Ok(())},
        _=>{usage();bail!("unknown command: {sub}")}
    }
}
