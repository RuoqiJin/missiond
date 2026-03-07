import { NextRequest, NextResponse } from 'next/server';
import { callTool } from '@/lib/missiond';

// Per-type limits for stratified sampling — prevents high-volume types from starving others
const TYPE_LIMITS: Record<string, number> = {
  gemini_request_started: 30, // vision worker generates thousands; sample only
};
const DEFAULT_PER_TYPE_LIMIT = 80;

// All known timeline event types grouped by swimlane
const ALL_EVENT_TYPES = [
  'user_message', 'assistant_message', 'thinking_message',           // Chat
  'gemini_request_started', 'gemini_request_completed',              // AI/LLM
  'decision_made', 'insight_generated',
  'codex_request_started', 'codex_request_completed',                // GPT
  'git_commit',                                                      // Code
  'task_lifecycle', 'question_created', 'question_resolved',         // Flow
  'board_task_created', 'board_task_status_changed',                 // Board
  'board_task_note_added', 'board_task_claimed',
  'board_task_deleted', 'board_task_updated',
  'slot_state_changed', 'memory_phase_changed',                      // System
  'translation_started', 'translation_completed', 'translation_failed', // Translation Worker
];

/** Convert window string to absolute ISO datetimes — avoids relative time parsing issues in backend */
function windowToAbsoluteRange(w: string): { since: string; until: string } {
  const until = new Date();
  let ms = 3600_000; // default 1h
  const minMatch = w.match(/^(\d+)min$/);
  if (minMatch) ms = parseInt(minMatch[1], 10) * 60_000;
  const hMatch = w.match(/^(\d+)h$/);
  if (hMatch) ms = parseInt(hMatch[1], 10) * 3600_000;
  const dMatch = w.match(/^(\d+)d$/);
  if (dMatch) ms = parseInt(dMatch[1], 10) * 86400_000;
  const since = new Date(until.getTime() - ms);
  return {
    since: since.toISOString().replace('T', ' ').slice(0, 19),
    until: until.toISOString().replace('T', ' ').slice(0, 19),
  };
}

export async function GET(req: NextRequest) {
  try {
    const eventType = req.nextUrl.searchParams.get('eventType') || undefined;
    const traceId = req.nextUrl.searchParams.get('traceId') || undefined;
    const windowStr = req.nextUrl.searchParams.get('window') || '24h';
    const query = req.nextUrl.searchParams.get('query') || undefined;
    const limitStr = req.nextUrl.searchParams.get('limit');
    // Support direct absolute time params (from gesture-driven range)
    const directSince = req.nextUrl.searchParams.get('since');
    const directUntil = req.nextUrl.searchParams.get('until');
    const { since, until } = directSince && directUntil
      ? { since: directSince, until: directUntil }
      : windowToAbsoluteRange(windowStr);

    // Use search if query provided
    if (query) {
      const limit = limitStr ? parseInt(limitStr, 10) : 200;
      const results = await callTool('mission_timeline_search', { query, limit, since, until });
      return NextResponse.json({ events: results });
    }

    // Single-type or trace query — pass through directly
    if (eventType || traceId) {
      const limit = limitStr ? parseInt(limitStr, 10) : 200;
      const args: Record<string, unknown> = { since, until, limit };
      if (eventType) args.eventType = eventType;
      if (traceId) args.traceId = traceId;
      const events = await callTool('mission_timeline_query', args);
      return NextResponse.json({ events, _range: { since, until } });
    }

    // Stratified fetch: parallel per-type calls to avoid high-volume types starving others
    const calls = ALL_EVENT_TYPES.map(async (type) => {
      const limit = TYPE_LIMITS[type] ?? DEFAULT_PER_TYPE_LIMIT;
      const result = await callTool('mission_timeline_query', {
        since, until, eventType: type, limit,
      }) as { events?: unknown[] };
      return result?.events ?? [];
    });

    const perTypeResults = await Promise.all(calls);
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const allEvents = perTypeResults.flat() as any[];
    // Sort by seq descending (newest first) — consistent with non-stratified mode
    allEvents.sort((a, b) => (b.seq ?? 0) - (a.seq ?? 0));

    // Fetch session metadata (startedAt) for capsule continuity across time windows
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const sessionIds = new Set<string>();
    for (const ev of allEvents) {
      if (ev.payload?.session_id && !ev.payload?.slot_id) {
        sessionIds.add(ev.payload.session_id);
      }
    }
    let sessions: Record<string, { startedAt: string }> = {};
    if (sessionIds.size > 0) {
      try {
        const convList = await callTool('mission_conversation_list', {
          conversationType: 'all', limit: 50,
        }) as Array<{ id: string; startedAt: string }>;
        if (Array.isArray(convList)) {
          for (const conv of convList) {
            if (sessionIds.has(conv.id)) {
              sessions[conv.id] = { startedAt: conv.startedAt };
            }
          }
        }
      } catch { /* session metadata is optional — capsules still work without it */ }
    }

    return NextResponse.json({
      events: { count: allEvents.length, offset: 0, events: allEvents },
      sessions,
      _range: { since, until },
    });
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 502 });
  }
}
