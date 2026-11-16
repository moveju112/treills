import { describe, expect, it } from 'vitest';
import { layoutGraph } from './graphLayout';
import type { AgentNode } from './contracts.generated';
const base: AgentNode = { id: 'main', workflow_id: 'w', parent_id: null, child_session_id: null, role: 'main', model: null, state: 'recent', started_at: null, ended_at: null, latest_at: null, last_tool: null, evidence: [] };
describe('agent graph', () => {
  it('keeps parallel children on separate positions', () => {
    const nodes = [base, { ...base, id: 'a', parent_id: 'main' }, { ...base, id: 'b', parent_id: 'main' }];
    const positions = layoutGraph(nodes, [{ id: '1', from: 'main', to: 'a', evidence_id: 'e1' }, { id: '2', from: 'main', to: 'b', evidence_id: 'e2' }]);
    expect(positions.get('a')?.x).not.toBe(positions.get('b')?.x);
    expect(positions.get('a')?.y).toBe(positions.get('b')?.y);
  });
  it('does not invent relationships for isolated nodes', () => {
    const positions = layoutGraph([base, { ...base, id: 'orphan' }], []);
    expect(positions.size).toBe(2);
  });
});
