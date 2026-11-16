import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { AgentNode, Project, SessionRecord, TimelineEvent, Workflow, WorkflowDetail } from './contracts.generated';
import type { ActivityProvider } from './provider';
import { Graph } from './Graph';
import { elapsed, stamp, stateText } from './status';
import styles from './Dashboard.module.css';

export type ActivityScope =
  | { kind: 'all' }
  | { kind: 'project'; id: string }
  | { kind: 'session'; id: string; projectId?: string }
  | { kind: 'execution'; id: string; sessionId?: string; projectId?: string };

export interface ActivityViewProps {
  provider: ActivityProvider;
  scope: ActivityScope;
  onScopeChange?: (next: ActivityScope) => void;
  selectedNodeId?: string | null;
  onNodeSelect?: (id: string | null) => void;
}
const PAGE = 30;

function ScopeBadge({ state }: {state: Workflow['state']}) {
  return <span className={styles.badge} data-status={state}><span aria-hidden="true">●</span> {stateText[state]}</span>;
}
function summary(project: Project) {
  return <div className={styles.projectStats}>
    <span>최근 활동 <b>{project.active_count}</b></span>
    <span>확인 필요 <b>{project.attention_count}</b></span>
    <span>종료 <b>{project.ended_count}</b></span>
  </div>;
}
function mergeEvents(old: TimelineEvent[], newer: TimelineEvent[]) {
  const events = new Map(old.map(e => [e.id,e]));
  newer.forEach(e => events.set(e.id,e));
  return [...events.values()].sort((a,b) => (a.at ?? Infinity)-(b.at ?? Infinity)||a.id.localeCompare(b.id));
}

export function ActivityView({ provider, scope, onScopeChange, selectedNodeId, onNodeSelect }: ActivityViewProps) {
  const [selection, setSelection] = useState<string | null>(null);
  const node = selectedNodeId === undefined ? selection : selectedNodeId;
  const selectNode = useCallback((id:string|null) => {
    setSelection(id); onNodeSelect?.(id);
  },[onNodeSelect]);
  const [projects, setProjects] = useState<Project[]>([]);
  const [sessions, setSessions] = useState<SessionRecord[]>([]);
  const [executions, setExecutions] = useState<Workflow[]>([]);
  const [detail, setDetail] = useState<WorkflowDetail|null>(null);
  const [events, setEvents] = useState<TimelineEvent[]>([]);
  const [eventCursor, setEventCursor] = useState<number|null>(0);
  const [next, setNext] = useState<number|null>(null);
  const [search,setSearch] = useState('');
  const [filter,setFilter] = useState<'all'|'active'|'attention'|'completed'>('all');
  const [connection,setConnection] = useState<'connecting'|'online'|'disconnected'>('connecting');
  const [error,setError] = useState<string|null>(null);
  const [issues,setIssues] = useState<string[]>([]);
  const [revision,setRevision] = useState(0);
  const changeCursor = useRef<number|null>(null);
  const scopeKind=scope.kind;
  const scopeId='id' in scope?scope.id:'';
  const scopeProjectId='projectId' in scope?scope.projectId:undefined;
  const scopeSessionId='sessionId' in scope?scope.sessionId:undefined;

  // Polling catches up using the change cursor. Cursor moves only after response success.
  useEffect(() => {
    let valid=true, busy=false;
    const tick=async()=>{
      if(busy)return;
      busy=true;
      try{
        if(changeCursor.current===null){
          const initial=await provider.listProjects({limit:1});
          if(!valid)return;
          changeCursor.current=initial.last_change_id;
          setRevision(i=>i+1);
        }else{
          let seen=changeCursor.current;
          let changed=false;
          for(let i=0;i<500;i++){
            const page=await provider.changes(seen);
            if(!valid)return;
            seen=page.next_cursor;
            changeCursor.current=seen;
            if(page.items.length)changed=true;
            if(!page.has_more)break;
          }
          if(changed)setRevision(i=>i+1);
        }
        setConnection('online');
      }catch(e){
        if(valid){setConnection('disconnected');setError(String(e));}
      }finally{busy=false;}
    };
    void tick();
    const timer=window.setInterval(()=>void tick(),2000);
    return ()=>{valid=false;window.clearInterval(timer);};
  },[provider]);

  useEffect(()=>{
    const controller=new AbortController();
    setError(null);
    setNext(null);
    const q = async()=>{
      try{
        if(scopeKind==='all'){
          const res=await provider.listProjects({filter,search,limit:PAGE},controller.signal);
          if(controller.signal.aborted)return;
          setProjects(res.items);setNext(res.next_offset);
          setIssues(res.issues.map(issue=>issue.path+': '+issue.message));
        }else if(scopeKind==='project'){
          const res=await provider.listSessions({projectId:scopeId,limit:PAGE},controller.signal);
          if(controller.signal.aborted)return;
          setSessions(res.items);setNext(res.next_offset);
        }else if(scopeKind==='session'){
          const res=await provider.listWorkflows({sessionId:scopeId,search,limit:PAGE},controller.signal);
          if(controller.signal.aborted)return;
          setExecutions(res.items);setNext(res.next_offset);
        }else{
          const res=await provider.getWorkflow(scopeId,controller.signal);
          if(controller.signal.aborted)return;
          setDetail(res);
        }
      }catch(e){if(!controller.signal.aborted)setError(String(e));}
    };
    void q();
    return ()=>controller.abort();
  },[provider,scopeKind,scopeId,search,filter,revision]);

  useEffect(()=>{
    if(scopeKind!=='execution'){setDetail(null);setEvents([]);setEventCursor(0);return;}
    let valid=true;
    selectNode(null);setEvents([]);setEventCursor(0);
    const load=async()=>{
      try{
        const page=await provider.listEvents(scopeId,0);
        if(valid){setEvents(page.items);setEventCursor(page.next_cursor);}
      }catch(e){if(valid)setError(String(e));}
    };
    void load();
    return ()=>{valid=false;};
  },[provider,scopeKind,scopeId]);

  const more=async()=>{
    if(next===null)return;
    try{
      if(scopeKind==='all'){
        const res=await provider.listProjects({filter,search,offset:next,limit:PAGE});
        setProjects(old=>[...old,...res.items]);setNext(res.next_offset);
      }else if(scopeKind==='project'){
        const res=await provider.listSessions({projectId:scopeId,offset:next,limit:PAGE});
        setSessions(old=>[...old,...res.items]);setNext(res.next_offset);
      }else if(scopeKind==='session'){
        const res=await provider.listWorkflows({sessionId:scopeId,search,offset:next,limit:PAGE});
        setExecutions(old=>[...old,...res.items]);setNext(res.next_offset);
      }
    }catch(e){setError(String(e));}
  };
  const moreEvents=async()=>{
    if(scopeKind!=='execution'||eventCursor===null)return;
    try{
      const page=await provider.listEvents(scopeId,eventCursor);
      setEvents(old=>mergeEvents(old,page.items));setEventCursor(page.next_cursor);
    }catch(e){setError(String(e));}
  };
  const focus:AgentNode|undefined=detail?.nodes.find(n=>n.id===node);
  const ordered=useMemo(()=>mergeEvents(events,detail?.events??[]),[events,detail]);
  const move=onScopeChange??(()=>{});
  const filterProject=(p:Project)=>{
    if(filter==='active'&&p.active_count===0)return false;
    if(filter==='attention'&&p.attention_count===0)return false;
    if(filter==='completed'&&p.ended_count===0)return false;
    return true;
  };
  return <section className={styles.activityView} data-scope={scopeKind}>
    <div className={styles.activityHeading}>
      <div><span className={styles.eyebrow}>TREILLS / {scopeKind.toUpperCase()}</span>
        <h2>{scopeKind==='all'?'전체 프로젝트':scopeKind==='project'?'프로젝트의 세션':scopeKind==='session'?'세션의 실행 / turns':'실행 관계'}</h2></div>
      <span className={styles.connection} data-connection={connection}>
        <span className={styles.connectionDot} aria-hidden="true"/>{connection==='online'?'동기화 중':connection==='connecting'?'연결 중':'연결 끊김'}
      </span>
    </div>
    {error&&<p className={styles.error} role="alert">조회 실패: {error}</p>}
    {issues.length>0&&<details className={styles.issue}><summary>수집 경고 {issues.length}건</summary>{issues.map((x,i)=><p key={i}>{x}</p>)}</details>}
    {(scopeKind==='all'||scopeKind==='session')&&<label className={styles.searchLabel}>이름 검색
      <input value={search} onChange={e=>setSearch(e.target.value)} placeholder="프로젝트 또는 실행 이름" type="search"/></label>}
    {scopeKind==='all'&&<div className={styles.filters} role="group" aria-label="프로젝트 상태">
      {(['all','active','attention','completed'] as const).map(f=>
        <button type="button" aria-pressed={filter===f} key={f} onClick={()=>setFilter(f)}>
          {{all:'전체',active:'최근 활동',attention:'확인 필요',completed:'종료 기록'}[f]}
        </button>)}</div>}
    {scopeKind==='all'&&<div className={styles.scopeGrid}>
      {projects.filter(filterProject).map(p=><button type="button" className={styles.projectCard} key={p.id}
        onClick={()=>move({kind:'project',id:p.id})}>
        <span className={styles.projectTop}><strong>{p.name}</strong><span>↗</span></span>
        <small title={p.root}>{p.root}</small>{summary(p)}
        <div className={styles.previews}>{p.previews.map(x=><div key={x.workflow_id} className={styles.preview}>
          ▣ {x.name} · {stateText[x.state]} · {x.node_count} nodes
        </div>)}</div><small>최근: {stamp(p.latest_at)}</small>
      </button>)}
      {!error&&projects.length===0&&<p className={styles.empty}>{search?'검색 결과가 없습니다.':'발견된 프로젝트가 없습니다.'}</p>}
    </div>}
    {scopeKind==='project'&&<div className={styles.scopeGrid}>
      {sessions.map(s=><button type="button" key={s.id} className={styles.projectCard}
         onClick={()=>move({kind:'session',id:s.id,projectId:s.project_id})}>
        <span className={styles.projectTop}><strong>{s.source}: {s.native_id}</strong><ScopeBadge state={s.state}/></span>
        <small title={s.worktree}>{s.worktree||'경로 미확인'}</small>
        {s.branch&&<small>branch: {s.branch}</small>}
        {s.parent_turn_id&&<small>위임 부모 실행: {s.parent_turn_id}</small>}
        {s.incomplete&&<small className={styles.warning}>불완전한 기록</small>}
        <small>최근: {stamp(s.latest_at)}</small>
      </button>)}
      {!error&&sessions.length===0&&<p className={styles.empty}>이 프로젝트에 수집된 세션이 없습니다.</p>}
    </div>}
    {scopeKind==='session'&&<div className={styles.scopeGrid}>
      {executions.map(x=><button type="button" key={x.id} className={styles.projectCard}
         onClick={()=>move({kind:'execution',id:x.id,sessionId:x.session_id,projectId:x.project_id})}>
        <span className={styles.projectTop}><strong>{x.name}</strong><ScopeBadge state={x.state}/></span>
        <small>{x.source} · turn: {x.turn_id} · {x.node_count} nodes</small>
        <small>시작: {stamp(x.started_at)} · 경과: {elapsed(x.started_at,x.ended_at)}</small>
        {x.parent_turn_id&&<small>부모 실행: {x.parent_turn_id}</small>}
        {x.incomplete&&<small className={styles.warning}>불완전한 기록</small>}
      </button>)}
      {!error&&executions.length===0&&<p className={styles.empty}>명시적으로 확인된 실행/turn이 없습니다. 세션 수준 이벤트와 혼동하지 않습니다.</p>}
    </div>}
    {next!==null&&scopeKind!=='execution'&&<button className={styles.more} onClick={()=>void more()}>이전 기록 더 보기</button>}
    {scopeKind==='execution'&&(!detail||detail.workflow.id!==scopeId?<p className={styles.empty}>실행 기록을 불러오는 중입니다.</p>:
      <div className={styles.executionLayout}>
        <div className={styles.detailHeader}><div><h2>{detail.workflow.name}</h2>
          <p>{detail.workflow.source} · {detail.workflow.session_id} · turn {detail.workflow.turn_id}</p>
          <p>{detail.workflow.worktree||'경로 미확인'}{detail.workflow.branch?' · '+detail.workflow.branch:''}</p>
          </div><div><ScopeBadge state={detail.workflow.state}/><small>{elapsed(detail.workflow.started_at,detail.workflow.ended_at)}</small></div></div>
        {detail.workflow.incomplete&&<p className={styles.warning}>과거 로그 일부가 불완전합니다.</p>}
        <Graph detail={detail} selectedNodeId={node} onSelect={selectNode} onDetail={()=>{}}/>
        <div className={styles.lowerPanels}>
          <section className={styles.nodeDetails} aria-label="노드 상세">
            <h3>NODE DETAILS</h3>
            {focus?<div>
              <div className={styles.detailRow}><span>역할</span><strong>{focus.role}</strong></div>
              <div className={styles.detailRow}><span>모델</span><strong>{focus.model??'미확인'}</strong></div>
              <div className={styles.detailRow}><span>상태</span><ScopeBadge state={focus.state}/></div>
              <div className={styles.detailRow}><span>실행 시간</span><span>{elapsed(focus.started_at,focus.ended_at)}</span></div>
              <div className={styles.detailRow}><span>시작</span><span>{stamp(focus.started_at)}</span></div>
              <div className={styles.detailRow}><span>종료</span><span>{stamp(focus.ended_at)}</span></div>
              <div className={styles.detailRow}><span>최근 도구</span><span>{focus.last_tool??'미확인'}</span></div>
              {focus.child_session_id&&<button className={styles.more} type="button" onClick={()=>move({kind:'session',id:focus.child_session_id!})}>자식 세션 보기</button>}
              <button className={styles.closeDetail} onClick={()=>selectNode(null)}>선택 해제</button>
            </div>:<p className={styles.muted}>노드를 선택하면 상세 내역이 표시됩니다.</p>}
          </section>
          <section className={styles.events} aria-label="시간순 이벤트">
            <div className={styles.eventsHeading}><h3>EVENT TIMELINE</h3><small>{ordered.length}개</small></div>
            <div className={styles.eventList}>{ordered.map(e=><div className={styles.eventRow} key={e.id}>
              <time>{stamp(e.at)}</time>
              <div><strong>{e.kind}</strong><span>{e.detail}</span><small>{e.evidence_source}{e.call_id?' · '+e.call_id:''}</small></div>
            </div>)}</div>
            {eventCursor!==null&&<button className={styles.more} onClick={()=>void moreEvents()}>이전 이벤트 더 보기</button>}
          </section>
        </div>
        <small className={styles.muted}>종료는 성공과 다릅니다. 상태는 원본 이벤트 근거에 따라 표시됩니다.</small>
      </div>)}
    {(scopeKind==='session'||scopeKind==='execution')&&<p className={styles.scopeNote}>
      선택된 범위만 서버에서 조회합니다. 출처가 불명확한 관계는 연결하지 않습니다.
      {scopeKind==='execution'&&scopeSessionId?' · '+scopeSessionId:''}{scopeProjectId?' · '+scopeProjectId:''}
    </p>}
  </section>;
}
