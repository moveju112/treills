import type { ChangesPage, EventsPage, ProjectsPage, SessionsPage, WorkflowDetail, WorkflowsPage } from './contracts.generated';

export interface ProjectQuery {
  filter?: 'all' | 'active' | 'attention' | 'completed';
  search?: string;
  offset?: number;
  limit?: number;
}
export interface SessionQuery {
  projectId?: string;
  source?: string;
  offset?: number;
  limit?: number;
}
export interface WorkflowQuery {
  projectId?: string;
  sessionId?: string;
  turnId?: string;
  state?: string;
  search?: string;
  offset?: number;
  limit?: number;
}
export interface ActivityProvider {
  listProjects(query: ProjectQuery, signal?: AbortSignal): Promise<ProjectsPage>;
  listSessions(query: SessionQuery, signal?: AbortSignal): Promise<SessionsPage>;
  listWorkflows(query: WorkflowQuery, signal?: AbortSignal): Promise<WorkflowsPage>;
  getWorkflow(id: string, signal?: AbortSignal): Promise<WorkflowDetail>;
  listEvents(id: string, cursor: number, signal?: AbortSignal): Promise<EventsPage>;
  listSessionEvents(id: string, cursor: number, signal?: AbortSignal): Promise<EventsPage>;
  changes(after: number, signal?: AbortSignal): Promise<ChangesPage>;
}
function params(input: Record<string,string | number | undefined>): string {
  const q = new URLSearchParams();
  Object.entries(input).forEach(([key, value]) => {
    if (value !== undefined && value !== '') q.set(key, String(value));
  });
  const s = q.toString();
  return s ? '?' + s : '';
}
export function createHttpProvider(base = ''): ActivityProvider {
  const urlBase = base.replace(/\/$/, '');
  async function request<T>(path: string, signal?: AbortSignal): Promise<T> {
    const response = await fetch(urlBase + path, { signal, headers: { Accept: 'application/json' }, credentials: 'same-origin' });
    if (!response.ok) throw new Error('HTTP ' + response.status + ': ' + response.statusText);
    const body: unknown = await response.json();
    if (typeof body !== 'object' || body === null || !('api_version' in body) ||
        typeof body.api_version !== 'string' || !body.api_version.startsWith('1.')) {
      throw new Error('Incompatible Treills API response');
    }
    return body as T;
  }
  return {
    listProjects: (q, signal) => request('/api/v1/projects' + params({
      filter: q.filter === 'all' ? undefined : q.filter, search: q.search, offset: q.offset, limit: q.limit,
    }), signal),
    listSessions: (q, signal) => request('/api/v1/sessions' + params({
      project_id: q.projectId, source: q.source, offset: q.offset, limit: q.limit,
    }), signal),
    listWorkflows: (q, signal) => request('/api/v1/workflows' + params({
      project_id: q.projectId, session_id: q.sessionId, turn_id: q.turnId,
      state: q.state, search: q.search, offset: q.offset, limit: q.limit,
    }), signal),
    getWorkflow: (id, signal) => request('/api/v1/workflows/' + encodeURIComponent(id), signal),
    listEvents: (id, cursor, signal) => request('/api/v1/workflows/' + encodeURIComponent(id) + '/events' + params({ cursor, limit: 100 }), signal),
    listSessionEvents: (id, cursor, signal) => request('/api/v1/sessions/' + encodeURIComponent(id) + '/events' + params({ cursor, limit: 100 }), signal),
    changes: (after, signal) => request('/api/v1/changes' + params({ after, limit: 200 }), signal),
  };
}
