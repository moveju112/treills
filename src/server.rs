use crate::{collector::{self, Sources}, domain::*, store};
use anyhow::{bail, Result};
use axum::{extract::{Path, Query, State},http::StatusCode,response::{IntoResponse,Response},routing::get,Json,Router};
use serde_json::json;
use std::{collections::HashMap,net::{IpAddr,SocketAddr},path::PathBuf,sync::Arc,time::Duration};
use tower_http::services::{ServeDir,ServeFile};
use tokio::net::TcpListener;

#[derive(Clone)]
struct AppState { db:PathBuf }
fn db(state:&AppState)->Result<rusqlite::Connection,ApiError>{
    rusqlite::Connection::open(&state.db).map_err(|e|ApiError::internal(e.to_string()))
}
struct ApiError {code:StatusCode,message:String}
impl ApiError {
    fn internal(message:String)->Self{Self{code:StatusCode::INTERNAL_SERVER_ERROR,message}}
    fn bad(message:&str)->Self{Self{code:StatusCode::BAD_REQUEST,message:message.into()}}
}
impl IntoResponse for ApiError {
    fn into_response(self)->Response{
        (self.code,Json(json!({"api_version":API_VERSION,"error":self.message}))).into_response()
    }
}
fn value(q:&HashMap<String,String>,key:&str,default:i64)->Result<i64,ApiError>{
    q.get(key).map(|s|s.parse::<i64>().map_err(|_|ApiError::bad("invalid integer query")))
      .transpose().map(|x|x.unwrap_or(default))
}
fn fail(e:anyhow::Error)->ApiError{ApiError::internal(e.to_string())}
async fn projects(State(s):State<Arc<AppState>>,Query(q):Query<HashMap<String,String>>)->Result<Json<ProjectsPage>,ApiError>{
    let d=db(&s)?;
    let page=store::projects(&d,q.get("filter").map(String::as_str),q.get("search").map(String::as_str),
       value(&q,"offset",0)?,value(&q,"limit",30)?).map_err(fail)?;
    Ok(Json(page))
}
async fn workflows(State(s):State<Arc<AppState>>,Query(q):Query<HashMap<String,String>>)->Result<Json<WorkflowsPage>,ApiError>{
    let d=db(&s)?;
    Ok(Json(store::workflows(&d,q.get("project_id").map(String::as_str),q.get("session_id").map(String::as_str),q.get("turn_id").map(String::as_str),q.get("state").map(String::as_str),
      q.get("search").map(String::as_str),value(&q,"offset",0)?,value(&q,"limit",30)?).map_err(fail)?))
}
async fn sessions(State(s):State<Arc<AppState>>,Query(q):Query<HashMap<String,String>>)->Result<Json<SessionsPage>,ApiError>{
    let d=db(&s)?;
    Ok(Json(store::sessions(&d,q.get("project_id").map(String::as_str),
       q.get("source").map(String::as_str),value(&q,"offset",0)?,value(&q,"limit",30)?).map_err(fail)?))
}
async fn session_events(State(s):State<Arc<AppState>>,Path(id):Path<String>,
    Query(q):Query<HashMap<String,String>>)->Result<Json<EventsPage>,ApiError>{
    let d=db(&s)?;
    Ok(Json(store::session_events(&d,&id,Some(value(&q,"cursor",0)?),
        value(&q,"limit",100)?).map_err(fail)?))
}

async fn workflow(State(s):State<Arc<AppState>>,Path(id):Path<String>)->Result<Response,ApiError>{
    let d=db(&s)?;
    match store::workflow(&d,&id).map_err(fail)?{
        Some(w)=>Ok(Json(w).into_response()),
        None=>Ok((StatusCode::NOT_FOUND,Json(json!({"api_version":API_VERSION,"error":"workflow not found"}))).into_response())
    }
}
async fn events(State(s):State<Arc<AppState>>,Path(id):Path<String>,Query(q):Query<HashMap<String,String>>)->Result<Json<EventsPage>,ApiError>{
    let d=db(&s)?;
    Ok(Json(store::events(&d,&id,Some(value(&q,"cursor",0)?),value(&q,"limit",100)?).map_err(fail)?))
}
async fn changes(State(s):State<Arc<AppState>>,Query(q):Query<HashMap<String,String>>)->Result<Json<ChangesPage>,ApiError>{
    let d=db(&s)?;
    Ok(Json(store::changes(&d,value(&q,"after",0)?,value(&q,"limit",100)?).map_err(fail)?))
}
async fn contract()->Json<serde_json::Value>{Json(json!({"api_version":API_VERSION,"poll_interval_ms":2000}))}
fn routes(state:Arc<AppState>)->Router{
    Router::new()
      .route("/api/v1/contract",get(contract))
      .route("/api/v1/projects",get(projects))
      .route("/api/v1/workflows",get(workflows))
      .route("/api/v1/sessions",get(sessions))
      .route("/api/v1/sessions/:id/events",get(session_events))
      .route("/api/v1/workflows/:id",get(workflow))
      .route("/api/v1/workflows/:id/events",get(events))
      .route("/api/v1/changes",get(changes))
      .with_state(state)
}
pub fn app(db_path:PathBuf,assets:&std::path::Path)->Router{
    let state=Arc::new(AppState{db:db_path});
    let index=assets.join("index.html");
    routes(state).fallback_service(ServeDir::new(assets).fallback(ServeFile::new(index)))
}
pub fn serve(bind:&str,port:u16,assets:Option<&str>)->Result<()>{
    let addr:IpAddr=bind.parse()?;
    if !addr.is_loopback(){
        eprintln!("WARNING: non-loopback binding exposes session metadata without authentication. Use SSH port forwarding.");
    }
    let assets=assets.map(PathBuf::from).unwrap_or_else(||PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("web/dist"));
    if !assets.join("index.html").is_file(){
        bail!("web build missing at {}; run 'cd web && npm ci && npm run build' first",assets.display());
    }
    let sources=Sources::default();
    let mut db=store::open()?;
    let report=collector::refresh(&mut db,&sources)?;
    eprintln!("collected {} events from {} changed files ({} issues)",report.events,report.files,report.issues);
    drop(db);
    let db_path=store::db_path();
    let app=app(db_path,&assets);
    let addr=SocketAddr::new(addr,port);
    let rt=tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    rt.block_on(async move {
        let sources=sources.clone();
        tokio::spawn(async move {
            let mut interval=tokio::time::interval(Duration::from_secs(2));
            loop {
                interval.tick().await;
                let sources=sources.clone();
                if let Err(error)=tokio::task::spawn_blocking(move ||{
                    let mut d=store::open()?;
                    collector::refresh(&mut d,&sources)?;
                    Ok::<(),anyhow::Error>(())
                }).await.unwrap_or_else(|e|Err(anyhow::anyhow!(e))) {
                    eprintln!("collector error: {error:#}");
                }
            }
        });
        let listener=TcpListener::bind(addr).await?;
        eprintln!("Treills dashboard: http://{addr}");
        axum::serve(listener,app).await?;
        Ok::<(),anyhow::Error>(())
    })
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test] fn safe_default_bind(){
        assert!("127.0.0.1".parse::<IpAddr>().unwrap().is_loopback());
    }
}
