import '@xyflow/react/dist/style.css';
import { useMemo, type KeyboardEvent } from 'react';
import {
  Background, BackgroundVariant, MarkerType, ReactFlow, ReactFlowProvider,
  useReactFlow, type Edge, type Node,
} from '@xyflow/react';
import type { AgentNode, WorkflowDetail } from './contracts.generated';
import { AgentBox, type BoxData } from './AgentBox';
import { layoutGraph } from './graphLayout';
import { stateText, stamp, elapsed } from './status';
import styles from './Dashboard.module.css';

const nodeTypes = { agentBox: AgentBox };
const active = (node: AgentNode) => node.state === 'running';
export function graphElements(detail: WorkflowDetail, selected: string | null) {
  const positions = layoutGraph(detail.nodes, detail.edges);
  const lookup = new Map(detail.nodes.map(n => [n.id, n]));
  const nodes: Node<BoxData, 'agentBox'>[] = detail.nodes.map(agent => ({
    id: agent.id, type: 'agentBox', position: positions.get(agent.id) ?? { x: 0, y: 0 },
    selected: selected === agent.id, data: { agent, active: active(agent) },
    draggable: false, connectable: false, deletable: false, focusable: true,
  }));
  const edges: Edge[] = detail.edges.filter(e => lookup.has(e.from) && lookup.has(e.to))
    .map(edge => ({
      id: edge.id, source: edge.from, target: edge.to, type: 'smoothstep',
      animated: active(lookup.get(edge.from)!) && active(lookup.get(edge.to)!),
      markerEnd: { type: MarkerType.ArrowClosed, color: 'var(--tr-edge)' },
      style: { stroke: 'var(--tr-edge)', strokeWidth: 1.5 },
    }));
  return { nodes, edges };
}
type GraphProps = {
  detail: WorkflowDetail;
  selectedNodeId: string | null;
  onSelect: (id: string | null) => void;
  onDetail: () => void;
};
function Canvas({ detail, selectedNodeId, onSelect, onDetail }: GraphProps) {
  const { fitView, zoomIn, zoomOut } = useReactFlow();
  const { nodes, edges } = useMemo(() => graphElements(detail, selectedNodeId), [detail, selectedNodeId]);
  const selected = detail.nodes.find(n => n.id === selectedNodeId);
  const key = (event: KeyboardEvent) => {
    if (event.key === 'Escape') onSelect(null);
    if (event.key === 'Enter' || event.key === ' ') {
      const target = event.target as HTMLElement;
      const id = target.closest('.react-flow__node')?.getAttribute('data-id');
      if(id){event.preventDefault();onSelect(id);onDetail();}
    }
  };
  return (
    <section className={styles.graphSection} aria-label="에이전트 실행 관계" onKeyDown={key}>
      <div className={styles.graphToolbar}>
        <div><strong>Agent graph</strong><span>{detail.nodes.length} agents · {detail.edges.length} verified links</span></div>
        <div className={styles.graphButtons}>
          <button type="button" onClick={() => zoomOut({ duration: 180 })} aria-label="축소">−</button>
          <button type="button" onClick={() => zoomIn({ duration: 180 })} aria-label="확대">+</button>
          <button type="button" onClick={() => fitView({ duration: 240, padding: 0.2 })}>화면 맞춤</button>
        </div>
      </div>
      <div className={styles.graphViewport} data-testid="graph-viewport">
        <ReactFlow
          nodes={nodes} edges={edges} nodeTypes={nodeTypes} fitView
          minZoom={0.2} maxZoom={2.2} nodesDraggable={false}
          nodesConnectable={false} elementsSelectable
          panOnScroll zoomOnPinch zoomOnScroll
          onNodeClick={(_, node) => { onSelect(node.id); onDetail(); }}
          onNodeDoubleClick={(_, node) => { onSelect(node.id); onDetail(); }}
          onPaneClick={() => onSelect(null)}
          proOptions={{ hideAttribution: true }}
        >
          <Background color="var(--tr-grid)" variant={BackgroundVariant.Dots} gap={24} size={1} />
        </ReactFlow>
      </div>
      {detail.edges.length === 0 && detail.nodes.length > 1 && (
        <p className={styles.graphNotice}>위임 관계의 근거가 없어 연결선을 표시하지 않았습니다.</p>
      )}
      {selected && <div className={styles.inlineSelection} aria-live="polite">
        선택: {selected.role} · {stateText[selected.state]} · {stamp(selected.latest_at)} · {elapsed(selected.started_at, selected.ended_at)}
      </div>}
    </section>
  );
}
export function Graph(props: GraphProps) {
  return <ReactFlowProvider><Canvas {...props} /></ReactFlowProvider>;
}
