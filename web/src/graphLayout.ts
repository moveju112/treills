import dagre from '@dagrejs/dagre';
import type { AgentNode, GraphEdge } from './contracts.generated';
export type Position = { x: number; y: number };
export const NODE_WIDTH = 228;
export const NODE_HEIGHT = 126;

export function layoutGraph(nodes: AgentNode[], edges: GraphEdge[]): Map<string, Position> {
  const graph = new dagre.graphlib.Graph();
  graph.setGraph({ rankdir: 'TB', nodesep: 48, ranksep: 92, marginx: 36, marginy: 36 });
  graph.setDefaultEdgeLabel(() => ({}));
  nodes.forEach(node => graph.setNode(node.id, { width: NODE_WIDTH, height: NODE_HEIGHT }));
  const ids = new Set(nodes.map(n => n.id));
  edges.forEach(edge => {
    if (ids.has(edge.from) && ids.has(edge.to) && edge.from !== edge.to) {
      graph.setEdge(edge.from, edge.to);
    }
  });
  dagre.layout(graph);
  return new Map(nodes.map(node => {
    const { x, y } = graph.node(node.id) as Position;
    return [node.id, { x: x - NODE_WIDTH / 2, y: y - NODE_HEIGHT / 2 }];
  }));
}
