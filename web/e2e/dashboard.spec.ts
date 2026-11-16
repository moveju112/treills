import { expect, test, type Page } from '@playwright/test';

const now = Math.floor(Date.now() / 1000);
const alpha = 'p:alpha';
const beta = 'p:beta';
const session = 's:claude:aa';
const run = 't:claude:abc:turn-1';
const workflow = {
  id: run, project_id: alpha, source: 'claude', session_id: session, turn_id: 'turn-1',
  parent_turn_id: null, name: '모니터링 UI 구현', state: 'recent',
  started_at: now - 300, ended_at: null, latest_at: now, worktree: '/repo/alpha',
  branch: 'feature/web', model: 'sonnet', node_count: 3, incomplete: false, evidence: ['e1'],
};
const node = (id: string, role: string, parent_id: string | null) => ({
  id, workflow_id: run, parent_id, child_session_id: null, role, model: 'sonnet',
  state: 'recent', started_at: now - 300, ended_at: null, latest_at: now,
  last_tool: role === 'reviewer' ? 'Read' : null, evidence: ['e1'],
});
const mainId = run + ':main';
const reviewer = run + ':agent:a';
const explorer = run + ':agent:b';
const nodes = [node(mainId, 'main', null), node(reviewer, 'reviewer', mainId), node(explorer, 'explorer', mainId)];
const edges = [
  { id: 'edge1', from: mainId, to: reviewer, evidence_id: 'e1' },
  { id: 'edge2', from: mainId, to: explorer, evidence_id: 'e2' },
];
const events = [{
  id: 'e1', workflow_id: run, session_id: session, turn_id: 'turn-1',
  node_id: mainId, at: now, kind: 'tool_start', detail: 'Read',
  evidence_source: 'claude', call_id: 'call-1',
}];

async function mockApi(page: Page, requests: string[]) {
  await page.route('**/api/v1/**', async route => {
    const url = new URL(route.request().url());
    const pathname = decodeURIComponent(url.pathname);
    requests.push(pathname + url.search);
    const wrap = (value: Record<string, unknown>) => ({ api_version: '1.0', ...value });
    let response: unknown;
    if (pathname.endsWith('/projects')) {
      response = wrap({
        items: [
          { id: alpha, name: '프로젝트 알파', root: '/repo/alpha', active_count: 1, attention_count: 0, ended_count: 0, latest_at: now, previews: [{ workflow_id: run, name: workflow.name, state: 'recent', node_count: 3 }] },
          { id: beta, name: '프로젝트 베타', root: '/repo/beta', active_count: 0, attention_count: 1, ended_count: 1, latest_at: now, previews: [] },
        ], next_offset: null, last_change_id: 0, issues: [],
      });
    } else if (pathname.endsWith('/sessions')) {
      response = wrap({ items: [
        { id: session, project_id: alpha, source: 'claude', native_id: 'aa', parent_turn_id: null,
          state: 'recent', started_at: now-300, ended_at: null, latest_at: now,
          worktree: '/repo/alpha', branch: 'feature/web', incomplete: false, evidence: [] },
      ], next_offset: null });
    } else if (pathname.endsWith('/workflows')) {
      const projectOk = !url.searchParams.get('project_id') || url.searchParams.get('project_id') === alpha;
      const sessionOk = !url.searchParams.get('session_id') || url.searchParams.get('session_id') === session;
      response = wrap({ items: projectOk && sessionOk ? [workflow] : [], next_offset: null });
    } else if (pathname.endsWith('/events')) {
      response = wrap({ items: events, next_cursor: null });
    } else if (pathname.endsWith('/changes')) {
      response = wrap({ items: [], next_cursor: Number(url.searchParams.get('after') ?? 0), has_more: false });
    } else if (pathname.endsWith('/' + run)) {
      response = wrap({ workflow, nodes, edges, events });
    } else if (pathname.endsWith('/contract')) {
      response = wrap({ poll_interval_ms: 2000 });
    } else {
      await route.fulfill({ status: 404, json: wrap({ error: 'unknown route' }) }); return;
    }
    await route.fulfill({ status: 200, json: response });
  });
}
test('all → project → session → turn preserves execution ID, connections and node selection', async ({ page }) => {
  const requests: string[] = [];
  await mockApi(page, requests);
  await page.goto('/');
  await expect(page.getByText('프로젝트 알파').first()).toBeVisible();
  await page.getByRole('button', { name: /프로젝트 알파/ }).first().click();
  await expect(page.getByText('프로젝트의 세션')).toBeVisible();
  await expect(page.getByText('실행 기록 / turns')).toBeVisible();
  await page.getByRole('button', { name: /claude: aa/ }).click();
  await expect(page.getByText('세션의 실행 / turns')).toBeVisible();
  await page.getByRole('button', { name: /모니터링 UI 구현/ }).click();
  await expect(page.getByText('Agent graph')).toBeVisible();
  await expect(page.locator('.react-flow__edge')).toHaveCount(2);
  const target = page.locator('.react-flow__node').filter({ hasText: 'reviewer' });
  await target.click();
  await expect(page.locator('section[aria-label="노드 상세"]')).toContainText('reviewer');
  await expect(page.locator('section[aria-label="노드 상세"]')).toContainText('Read');
  const viewport = page.locator('.react-flow__viewport');
  const original = await viewport.getAttribute('style');
  await page.getByRole('button', { name: '확대' }).click();
  await expect.poll(async () => viewport.getAttribute('style')).not.toBe(original);
  await page.getByRole('button', { name: '화면 맞춤' }).click();
  await expect(page.locator('section[aria-label="노드 상세"]')).toContainText('reviewer');
  expect(requests.some(q => q.includes('project_id=' + alpha))).toBe(true);
  expect(requests.some(q => q.includes('session_id=' + session))).toBe(true);
});
test('mobile graph pan/zoom stays inside viewport', async ({ page }) => {
  const requests: string[] = [];
  await page.setViewportSize({ width: 390, height: 844 });
  await mockApi(page, requests);
  await page.goto('/');
  await page.getByRole('button', { name: /프로젝트 알파/ }).first().click();
  await page.getByRole('button', { name: /모니터링 UI 구현/ }).first().click();
  await expect(page.locator('[data-testid="graph-viewport"]')).toBeVisible();
  await expect.poll(async () => page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
  await page.locator('.react-flow__node').filter({ hasText: 'reviewer' }).click();
  await expect(page.locator('section[aria-label="노드 상세"]')).toContainText('reviewer');
  await page.screenshot({ path: 'test-results/mobile-dashboard.png', fullPage: true });
});
