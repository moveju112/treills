import { Handle, Position, type NodeProps, type Node } from '@xyflow/react';
import type { AgentNode } from './contracts.generated';
import { elapsed, stateText } from './status';
import styles from './Dashboard.module.css';

export type BoxData = { agent: AgentNode; active: boolean } & Record<string, unknown>;
export function AgentBox({ data, selected }: NodeProps<Node<BoxData, 'agentBox'>>) {
  const { agent } = data as BoxData;
  return (
    <div className={styles.agentBox} data-status={agent.state} data-selected={selected} tabIndex={0}
      role="group" aria-label={agent.role + ', ' + stateText[agent.state]}>
      <Handle type="target" position={Position.Top} className={styles.handle} isConnectable={false} />
      <div className={styles.agentHeader}>
        <span className={styles.agentRole} title={agent.role}>{agent.role}</span>
        <span className={styles.dot} data-status={agent.state} aria-hidden="true" />
      </div>
      <div className={styles.agentModel}>{agent.model || '모델 미확인'}</div>
      <div className={styles.agentStatus}>{stateText[agent.state]} · {elapsed(agent.started_at, agent.ended_at)}</div>
      <div className={styles.agentTool} title={agent.last_tool || ''}>{agent.last_tool ? '도구: ' + agent.last_tool : '최근 도구 정보 없음'}</div>
      <Handle type="source" position={Position.Bottom} className={styles.handle} isConnectable={false} />
    </div>
  );
}
