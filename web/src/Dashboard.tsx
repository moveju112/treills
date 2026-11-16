import { useState } from 'react';
import { ActivityView, type ActivityScope } from './ActivityView';
import type { ActivityProvider } from './provider';
import styles from './Dashboard.module.css';

export interface DashboardProps {
  provider: ActivityProvider;
  scope?: ActivityScope;
  onScopeChange?: (scope: ActivityScope) => void;
}
export function Dashboard({ provider, scope, onScopeChange }: DashboardProps) {
  const [stack, setStack] = useState<ActivityScope[]>([{kind:'all'}]);
  const current = scope ?? stack[stack.length - 1];
  function go(next:ActivityScope) {
    if (scope) {onScopeChange?.(next);return;}
    setStack(old=>[...old,next]);
    onScopeChange?.(next);
  }
  function back() {
    if (scope) {onScopeChange?.({kind:'all'});return;}
    setStack(old=>old.length>1?old.slice(0,-1):old);
  }
  return <div className={styles.dashboard}>
    <header className={styles.header}>
      <div className={styles.brand}><span className={styles.logo}>T</span><div><strong>TREILLS</strong><small>Agent Activity Explorer · API v1</small></div></div>
      <span className={styles.version}>Claude Code / Codex</span>
    </header>
    <nav className={styles.navigation}>
      <button type="button" onClick={back} disabled={current.kind==='all'}>← 이전</button>
      <span>/</span><strong>{current.kind === 'all' ? '전체 프로젝트' : current.kind === 'project' ? '프로젝트' : current.kind === 'session' ? '세션' : '실행'}</strong>
    </nav>
    <ActivityView provider={provider} scope={current} onScopeChange={go}/>
  </div>;
}
