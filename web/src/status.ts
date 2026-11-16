import type { RunState } from './contracts.generated';
export const stateText: Record<RunState, string> = {
  unknown: '확인 불가', running: '실행 확인', recent: '최근 활동', waiting: '대기',
  inactive: '비활성', success: '성공', failed: '실패', cancelled: '취소', ended: '종료',
};
export function stamp(ts: number | null | undefined) {
  return ts == null ? '시간 미확인' : new Date(ts * 1000).toLocaleString();
}
export function elapsed(start: number | null, end: number | null, now = Date.now() / 1000) {
  if (start == null) return '시작 시간 미확인';
  const seconds = Math.max(0, Math.floor((end ?? now) - start));
  if (seconds < 60) return seconds + '초';
  if (seconds < 3600) return Math.floor(seconds / 60) + '분';
  if (seconds < 86400) return Math.floor(seconds / 3600) + '시간 ' + Math.floor(seconds % 3600 / 60) + '분';
  return Math.floor(seconds / 86400) + '일';
}
