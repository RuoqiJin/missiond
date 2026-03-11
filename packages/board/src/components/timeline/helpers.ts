import { beijingDayRange, toBeijingDate } from '@/lib/time';
import type { TimelineEvent, SelectionState, DotVisualStatus, CapsuleVisualStatus } from './types';
import { EVENT_COLORS, SLOT_COLORS, SWIMLANES, SLOT_LANE_IDX, SESSION_COLORS, STORAGE_KEY } from './constants';

// ── Visual Status Derivation ──

export function getDotStatus(ev: TimelineEvent, sel: SelectionState, contextSeqSet: Set<number>): DotVisualStatus {
  if (sel.focusedSeq === ev.seq) return 'focused';
  if (sel.scope === 'trace' && contextSeqSet.has(ev.seq)) return 'highlighted';
  if (sel.scope === 'session' && ev.payload?.session_id === sel.scopeId) return 'highlighted';
  if (sel.scope === 'slot' && ev.payload?.slot_id === sel.scopeId) return 'highlighted';
  if (sel.scope !== 'global') return 'dimmed';
  return 'normal';
}

export function getCapsuleStatus(sid: string, sel: SelectionState): CapsuleVisualStatus {
  if ((sel.scope === 'session' || sel.scope === 'slot') && sel.scopeId === sid) return 'selected';
  if (sel.scope !== 'global') return 'dimmed';
  return 'normal';
}

// ── Color Lookups ──

export function getEventColor(type: string) {
  return EVENT_COLORS[type] || { dot: 'bg-neutral-500', glow: 'shadow-neutral-500/50', bg: 'bg-neutral-500/10', text: 'text-neutral-400', label: type, icon: null };
}

export function getSlotColor(slotId: string | null) {
  if (!slotId) return null;
  return SLOT_COLORS[slotId] || { badge: 'bg-neutral-500/20 text-neutral-300', border: 'border-l-neutral-400' };
}

// ── Lane Logic ──

export function getSwimlane(ev: TimelineEvent): number {
  if (ev.payload?.slot_id) return SLOT_LANE_IDX;
  for (let i = 0; i < SWIMLANES.length; i++) {
    if (SWIMLANES[i].types.includes(ev.event_type)) return i;
  }
  return SWIMLANES.length - 1;
}

export function isLaneVisible(laneId: string, soloed: Set<string>, muted: Set<string>): boolean {
  if (muted.has(laneId)) return false;
  if (soloed.size > 0) return soloed.has(laneId);
  return true;
}

export function getEventLaneId(ev: TimelineEvent): string {
  if (ev.payload?.slot_id) return 'slot';
  for (const lane of SWIMLANES) {
    if (lane.types.includes(ev.event_type)) return lane.id;
  }
  return 'sys';
}

// ── Session Helpers ──

export function hashSessionColor(sessionId: string): number {
  let h = 0;
  for (let i = 0; i < sessionId.length; i++) {
    h = ((h << 5) - h + sessionId.charCodeAt(i)) | 0;
  }
  return ((h % SESSION_COLORS.length) + SESSION_COLORS.length) % SESSION_COLORS.length;
}

export function isChatEvent(type: string): boolean {
  return type === 'user_message' || type === 'assistant_message' || type === 'thinking_message';
}

// ── Event Summaries ──

export function eventSummary(ev: TimelineEvent): string {
  if (ev.summary) return ev.summary;
  const p = ev.payload;
  if (!p) return ev.event_type;
  switch (ev.event_type) {
    case 'slot_state_changed': return `${p.slot_id || ''} → ${p.new_state || ''}`;
    case 'task_lifecycle': return `${p.action || ''}: ${p.task_title || p.task_id || ''}`;
    case 'cli_request_started': return `[${p.engine || '?'}] ${p.caller || ''} → ${p.model || ''} (${p.prompt_chars || 0}ch)`;
    case 'cli_request_completed': return `[${p.engine || '?'}] ${p.caller || ''} ${p.duration_ms ? p.duration_ms + 'ms' : ''} ${p.error ? '❌' : '✓'}`;
    case 'cli_tool_activity': return `[${p.engine || '?'}] #${p.tool_seq || 0} ${p.activity || ''} ${p.tool_name || ''}`;
    case 'gemini_request_started': return `${p.caller || ''} → ${p.model || ''} (${p.prompt_chars || 0} chars)`;
    case 'gemini_request_completed': return `${p.caller || ''} ${p.duration_ms ? p.duration_ms + 'ms' : ''} ${p.error ? '❌' : ''}`;
    case 'codex_request_started': return `${p.caller || ''} → ${p.model || ''} (${p.prompt_chars || 0}ch${p.has_image ? ' +img' : ''})`;
    case 'codex_request_completed': return `${p.caller || ''} ${p.duration_ms ? p.duration_ms + 'ms' : ''} ${p.error ? '❌' : '✓'} ${p.output_tokens ? p.output_tokens + 'tok' : ''}`;
    case 'git_commit': return `${p.short_hash || ''} ${p.message || ''}`;
    case 'user_message': return `${p.preview || ''}`;
    case 'assistant_message': return `${p.preview || ''}`;
    case 'decision_made': return `${p.tier || ''}: ${p.question?.slice(0, 60) || ''}`;
    case 'insight_generated': return `${p.title || ''}`;
    case 'board_task_created': return `Created: ${p.title || ''}`;
    case 'board_task_status_changed': return `${p.old_status || ''} → ${p.new_status || ''}`;
    case 'board_task_note_added': return `Note: ${p.content_preview || ''}`;
    case 'board_task_claimed': return `Claimed by ${p.slot_id || ''}`;
    case 'board_task_deleted': return `Deleted: ${p.title || ''}`;
    case 'board_task_updated': return `${p.title || ''} → ${p.status || ''}`;
    case 'briefing_batch_started': return `Briefing: ${p.pending_count || 0} pending`;
    case 'briefing_summary_generated': return `seq=${p.target_seq}: ${p.summary || ''}`;
    case 'system_message': return `[Daemon] ${p.preview || ''}`;
    case 'slot_task_dispatched': return `→ ${p.slot_id || ''} [${p.purpose || ''}] ${p.preview || ''}`;
    case 'translation_started': return `Translating msg#${p.message_id} (${p.content_chars || 0}ch)`;
    case 'translation_completed': return `Translated msg#${p.message_id} (${p.duration_ms || 0}ms): ${p.preview || ''}`;
    case 'translation_failed': return `Translation failed msg#${p.message_id}: ${p.error || ''}`;
    default: return ev.event_type;
  }
}

export function abstractTaskStep(ev: TimelineEvent): { title: string; subtitle: string; intent: string } {
  const p = ev.payload;
  switch (ev.event_type) {
    case 'user_message':
      return { title: '接收用户指令', subtitle: p?.preview || ev.summary || '', intent: '接收并理解用户的原始需求' };
    case 'thinking_message':
      return { title: '思考执行策略', subtitle: p?.content_chars ? `${p.content_chars} chars` : '', intent: '规划下一步操作，评估上下文并决定调用哪些工具' };
    case 'assistant_message': {
      const toolMatch = p?.preview?.match(/^\[([\w_]+)\]$/);
      if (toolMatch) {
        const toolName = toolMatch[1];
        const toolMap: Record<string, { title: string; intent: string }> = {
          Read: { title: '读取文件', intent: '获取文件内容以便分析或修改' },
          Edit: { title: '修改文件', intent: '将代码修改应用到工作区文件' },
          Write: { title: '创建/覆写文件', intent: '创建新文件或完整重写' },
          Bash: { title: '执行终端命令', intent: '通过终端执行系统命令、构建或 Git 操作' },
          Grep: { title: '搜索代码', intent: '在代码库中查找相关引用或定义' },
          Glob: { title: '查找文件', intent: '按模式搜索文件路径' },
          Agent: { title: '启动子 Agent', intent: '将子任务委派给专门的 Agent 并行处理' },
          Skill: { title: '调用 Skill', intent: '执行预定义的技能模块' },
        };
        const info = toolMap[toolName] || { title: `调用 ${toolName}`, intent: '执行工具操作' };
        return { title: info.title, subtitle: '', intent: info.intent };
      }
      return { title: '回复', subtitle: p?.preview || '', intent: '向用户汇报进度或解释执行结果' };
    }
    case 'cli_request_started':
      return { title: `咨询 ${p?.engine || 'CLI'}`, subtitle: `${p?.caller || ''} → ${p?.model || ''}`, intent: '利用外部 CLI 引擎进行分析' };
    case 'cli_request_completed':
      return { title: `${p?.engine || 'CLI'} 返回`, subtitle: `${p?.duration_ms || 0}ms ${p?.error ? '❌' : '✓'}`, intent: 'CLI 引擎分析结果返回' };
    case 'gemini_request_started':
      return { title: '咨询 Gemini', subtitle: `${p?.caller || ''} → ${p?.model || ''}`, intent: '利用 Gemini 进行大规模上下文分析' };
    case 'gemini_request_completed':
      return { title: 'Gemini 返回', subtitle: `${p?.duration_ms || 0}ms ${p?.error ? '❌' : '✓'}`, intent: 'Gemini 分析结果返回' };
    case 'slot_state_changed':
      return { title: '工位状态变更', subtitle: `${p?.prev_state || ''} → ${p?.new_state || ''}`, intent: '推进任务流生命周期' };
    case 'slot_task_dispatched':
      return { title: '派发任务', subtitle: `→ ${p?.slot_id || ''} [${p?.purpose || ''}]`, intent: '将子任务分配给工位执行' };
    case 'git_commit':
      return { title: 'Git 提交', subtitle: `${p?.short_hash || ''} ${p?.message || ''}`, intent: '将代码变更持久化到版本控制' };
    case 'memory_phase_changed':
      return { title: '记忆阶段变更', subtitle: ev.summary || '', intent: '记忆提取系统状态转换' };
    case 'board_task_created':
      return { title: '创建 Board 任务', subtitle: p?.title || '', intent: '将发现的问题或工作项记录到任务板' };
    case 'board_task_status_changed':
      return { title: '任务状态变更', subtitle: `${p?.old_status || ''} → ${p?.new_status || ''}`, intent: '更新任务完成状态' };
    case 'board_task_claimed':
      return { title: '认领任务', subtitle: `by ${p?.slot_id || ''}`, intent: '工位开始执行指定任务' };
    case 'insight_generated':
      return { title: '生成洞察', subtitle: p?.title || '', intent: 'Timeline 分析师生成了可执行的改进建议' };
    case 'decision_made':
      return { title: '做出决策', subtitle: p?.question?.slice(0, 60) || '', intent: '决策引擎对问题做出裁决' };
    default:
      return { title: EVENT_COLORS[ev.event_type]?.label || ev.event_type, subtitle: ev.summary || '', intent: '' };
  }
}

export function hasError(ev: TimelineEvent): boolean {
  if (!ev.payload) return false;
  return !!ev.payload.error || !!ev.payload.error_msg || ev.payload.status === 'error';
}

export function shortTrace(id: string | null): string {
  if (!id) return '';
  return id.slice(0, 7);
}

// ── Time Window ──

export function windowToMs(w: string): number {
  const minMatch = w.match(/^(\d+)min$/);
  if (minMatch) return parseInt(minMatch[1], 10) * 60 * 1000;
  const hMatch = w.match(/^(\d+)h$/);
  if (hMatch) return parseInt(hMatch[1], 10) * 3600 * 1000;
  const dMatch = w.match(/^(\d+)d$/);
  if (dMatch) return parseInt(dMatch[1], 10) * 86400 * 1000;
  return 3600 * 1000;
}

// ── View State Persistence ──

export function loadViewState(): { activeWindow: string; dailyDate: string | null; soloed: string[]; muted: string[] } {
  try {
    const raw = globalThis.localStorage?.getItem(STORAGE_KEY);
    if (raw) {
      const parsed = JSON.parse(raw);
      return {
        activeWindow: parsed.activeWindow || '24h',
        dailyDate: parsed.dailyDate || null,
        soloed: Array.isArray(parsed.soloed) ? parsed.soloed : [],
        muted: Array.isArray(parsed.muted) ? parsed.muted : [],
      };
    }
  } catch { /* ignore */ }
  return { activeWindow: '24h', dailyDate: null, soloed: [], muted: [] };
}

export function saveViewState(state: { activeWindow: string; dailyDate: string | null; soloed: string[]; muted: string[] }) {
  try { globalThis.localStorage?.setItem(STORAGE_KEY, JSON.stringify(state)); } catch { /* ignore */ }
}

// ── Daily View Helpers ──

export function formatDailyLabel(dateStr: string): string {
  const [y, m, d] = dateStr.split('-').map(Number);
  const noon = Date.UTC(y, m - 1, d, 12) - 8 * 3600_000;
  return new Intl.DateTimeFormat('zh-CN', {
    month: 'numeric', day: 'numeric', weekday: 'short',
    timeZone: 'Asia/Shanghai',
  }).format(new Date(noon));
}

export function shiftDay(dateStr: string, delta: number): string {
  const { start } = beijingDayRange(dateStr);
  return toBeijingDate(start + delta * 86400_000);
}
