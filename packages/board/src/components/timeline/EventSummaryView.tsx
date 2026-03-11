import { useState, useEffect } from 'react';
import {
  Sparkles, AlertTriangle, Zap, Brain, Wrench, ArrowRight,
  ChevronDown, ChevronRight, MessageSquare, GitCommit, Activity,
  Cpu, Settings2, User, Clock,
} from 'lucide-react';
import { cn } from '@/lib/utils';
import { formatBeijing } from '@/lib/time';
import { useFullMessage } from '../../hooks/useFullMessage';
import type { TimelineEvent } from './types';
import { getEventColor, getSlotColor, hasError, shortTrace, eventSummary } from './helpers';
import { ContentBlocksRenderer } from './ToolViewers';
import { MarkdownContent } from './MarkdownContent';

// ── Summary Views (per event type) ───────────────────────────

export function EventSummaryView({ event }: { event: TimelineEvent }) {
  const p = event.payload;
  if (!p) return <p className="text-xs text-neutral-500 italic">No payload</p>;

  switch (event.event_type) {
    case 'user_message':
    case 'assistant_message':
      return <ChatSummary event={event} />;
    case 'thinking_message':
      return <ThinkingSummary event={event} />;
    case 'slot_state_changed':
      return <SlotStateSummary event={event} />;
    case 'task_lifecycle':
    case 'board_task_created':
    case 'board_task_status_changed':
    case 'board_task_note_added':
    case 'board_task_claimed':
    case 'board_task_deleted':
    case 'board_task_updated':
      return <TaskSummary event={event} />;
    case 'cli_request_started':
    case 'cli_request_completed':
    case 'cli_tool_activity':
    case 'gemini_request_started':
    case 'gemini_request_completed':
    case 'codex_request_started':
    case 'codex_request_completed':
      return <LlmRequestSummary event={event} />;
    case 'git_commit':
      return <GitCommitSummary event={event} />;
    case 'memory_phase_changed':
      return <MemoryPhaseSummary event={event} />;
    case 'decision_made':
      return <DecisionSummary event={event} />;
    case 'insight_generated':
      return <InsightSummary event={event} />;
    case 'question_created':
    case 'question_resolved':
      return <QuestionSummary event={event} />;
    case 'system_message':
      return <SystemMessageSummary event={event} />;
    default:
      return <DefaultSummary event={event} />;
  }
}

// ── Shared small components ──

export function StatCard({ label, value, color }: { label: string; value: string | number; color?: string }) {
  return (
    <div className="px-3 py-1.5 bg-neutral-900 border border-neutral-800 rounded-md">
      <div className="text-[10px] text-neutral-500">{label}</div>
      <div className={cn('text-sm font-mono font-medium', color || 'text-neutral-200')}>{value}</div>
    </div>
  );
}

export function MiniStat({ label, value }: { label: string; value: string }) {
  return (
    <div className="px-2 py-1.5 bg-neutral-900 rounded border border-neutral-800">
      <div className="text-[9px] text-neutral-500">{label}</div>
      <div className="text-[11px] text-neutral-300 font-mono truncate">{value}</div>
    </div>
  );
}

export function EventMeta({ event }: { event: TimelineEvent }) {
  const ec = getEventColor(event.event_type);
  return (
    <div className="space-y-3">
      {/* Header */}
      <div className="flex items-center gap-2">
        <div className={cn('w-3 h-3 rounded-full', ec.dot)} />
        <span className={cn('text-sm font-medium', ec.text)}>{ec.label}</span>
        {event.payload?.slot_id && (() => {
          const sc = getSlotColor(event.payload.slot_id);
          return sc ? <span className={cn('text-[10px] px-1.5 py-0.5 rounded', sc.badge)}>{event.payload.slot_id}</span> : null;
        })()}
        {hasError(event) && <AlertTriangle className="w-3 h-3 text-red-400" />}
      </div>

      {/* Summary */}
      {event.event_type === 'insight_generated' ? (
        <div className="p-2 rounded border border-emerald-500/30 bg-emerald-950/20">
          <div className="flex items-center gap-1.5 mb-1">
            <Sparkles className="w-3 h-3 text-emerald-400" />
            <span className="text-[10px] text-emerald-400 uppercase tracking-wider">Insight</span>
          </div>
          <p className="text-xs text-emerald-200">{eventSummary(event)}</p>
        </div>
      ) : (
        <p className="text-xs text-neutral-300">{eventSummary(event)}</p>
      )}

      {/* Meta fields */}
      <div className="space-y-1.5 text-[11px]">
        <MetaRow label="Time" value={formatBeijing(event.created_at)} />
        <MetaRow label="Seq" value={`#${event.seq}`} />
        {event.payload?.session_id && <MetaRow label="Session" value={shortTrace(event.payload.session_id)} mono />}
        {event.trace_id && <MetaRow label="Trace" value={shortTrace(event.trace_id)} mono />}
        {event.span_id && <MetaRow label="Span" value={shortTrace(event.span_id)} mono />}
        {event.parent_span_id && <MetaRow label="Parent" value={shortTrace(event.parent_span_id)} mono />}
      </div>
    </div>
  );
}

function MetaRow({ label, value, mono }: { label: string; value: string; mono?: boolean }) {
  return (
    <div className="flex justify-between">
      <span className="text-neutral-500">{label}</span>
      <span className={cn('text-neutral-300', mono && 'font-mono')}>{value}</span>
    </div>
  );
}

// ── Per-type summary components ──

function ChatSummary({ event }: { event: TimelineEvent }) {
  const { role, preview, content_chars, message_id } = event.payload || {};
  const isUser = role === 'user';
  const toolMatch = preview?.match(/^\[([\w_]+)\]$/);

  const fullMsg = useFullMessage(message_id, true);

  // Render structured content blocks for assistant messages with tool_use
  if (fullMsg?.contentBlocks && !isUser) {
    return (
      <div className="space-y-3">
        <div className="flex items-center gap-2">
          <span className="text-[10px] uppercase tracking-wider text-neutral-500 font-medium">Assistant Response</span>
          <span className="text-[10px] text-neutral-600 bg-neutral-900 px-1.5 py-0.5 rounded font-mono">{content_chars} chars</span>
        </div>
        <ContentBlocksRenderer blocks={fullMsg.contentBlocks} />
      </div>
    );
  }

  // User message with images: render text + inline images
  if (isUser && fullMsg && fullMsg.imageCount > 0) {
    // Extract text parts from contentBlocks (images are stripped to placeholders)
    const textParts = fullMsg.contentBlocks
      ?.filter((b: { type: string }) => b.type === 'text')
      .map((b: { text: string }) => b.text)
      .join('\n') || fullMsg.content;
    // Strip image placeholder text like [图片: image/png]
    const cleanText = textParts.replace(/\[图片: [\w/]+\]\n?/g, '').trim();

    return (
      <div className="space-y-3">
        <div className="flex items-center gap-2">
          <span className="text-[10px] uppercase tracking-wider text-neutral-500 font-medium">User Message</span>
          <span className="text-[10px] text-neutral-600 bg-neutral-900 px-1.5 py-0.5 rounded font-mono">{content_chars} chars</span>
        </div>
        {cleanText && (
          <div className="p-3 rounded-lg text-sm leading-relaxed bg-blue-500/10 border border-blue-500/20 text-blue-100 whitespace-pre-wrap break-words">
            {cleanText}
          </div>
        )}
        <div className="flex flex-wrap gap-2">
          {Array.from({ length: fullMsg.imageCount }, (_, i) => (
            <MessageImage key={i} messageId={message_id} index={i} />
          ))}
        </div>
      </div>
    );
  }

  const displayText = fullMsg?.content ?? preview;

  return (
    <div className="space-y-3">
      <div className="flex items-center gap-2">
        <span className="text-[10px] uppercase tracking-wider text-neutral-500 font-medium">
          {isUser ? 'User Message' : 'Assistant Response'}
        </span>
        <span className="text-[10px] text-neutral-600 bg-neutral-900 px-1.5 py-0.5 rounded font-mono">
          {content_chars} chars
        </span>
      </div>

      {toolMatch && !isUser && !fullMsg ? (
        <div className="inline-flex items-center gap-1.5 px-2.5 py-1.5 rounded-md bg-teal-500/10 border border-teal-500/20 text-teal-400 text-xs font-mono">
          <Wrench className="w-3 h-3" />
          <span>{toolMatch[1]}</span>
        </div>
      ) : displayText ? (
        <div className={cn(
          'p-3 rounded-lg text-sm leading-relaxed max-h-[600px] overflow-auto',
          isUser
            ? 'bg-blue-500/10 border border-blue-500/20 text-blue-100 whitespace-pre-wrap break-words'
            : 'bg-teal-500/10 border border-teal-500/20',
        )}>
          {isUser ? displayText : <MarkdownContent content={displayText} />}
        </div>
      ) : null}
    </div>
  );
}

/** Lazy-loaded image from a conversation message */
function MessageImage({ messageId, index }: { messageId: number; index: number }) {
  const [expanded, setExpanded] = useState(false);
  const src = `/api/system/message-image?message_id=${messageId}&index=${index}`;
  return (
    <div className="my-1">
      {/* eslint-disable-next-line @next/next/no-img-element */}
      <img
        src={src}
        alt={`Attachment ${index + 1}`}
        className={cn(
          'rounded-lg border border-neutral-700 cursor-pointer transition-all hover:border-neutral-500',
          expanded ? 'max-w-full' : 'max-w-sm max-h-64 object-cover',
        )}
        onClick={() => setExpanded(!expanded)}
        loading="lazy"
      />
    </div>
  );
}

function ThinkingSummary({ event }: { event: TimelineEvent }) {
  const preview = event.payload?.preview || '';
  const messageId = event.payload?.message_id;
  const contentChars = event.payload?.content_chars || 0;

  const fullMsg = useFullMessage(messageId, true);
  const displayText = fullMsg?.content ?? preview;
  const translation = fullMsg?.translation;
  const [showOriginal, setShowOriginal] = useState(false);

  return (
    <div className="border border-violet-500/20 rounded-lg bg-violet-500/5 overflow-hidden">
      <div className="flex items-center gap-2 p-2.5">
        <Brain className="w-4 h-4 text-violet-400 shrink-0" />
        <span className="text-xs text-violet-300 font-medium">Thinking</span>
        <span className="text-[10px] text-neutral-600 font-mono">{contentChars} chars</span>
        <div className="flex-1" />
        {translation && (
          <button
            onClick={() => setShowOriginal(!showOriginal)}
            className={cn(
              'text-[10px] px-1.5 py-0.5 rounded font-medium transition-colors',
              showOriginal ? 'bg-violet-500/20 text-violet-300' : 'bg-indigo-500/20 text-indigo-300',
            )}
          >
            {showOriginal ? 'EN' : '中'}
          </button>
        )}
      </div>
      <div className="px-3 pb-3 border-t border-violet-500/10">
        {translation && !showOriginal ? (
          <pre className="text-[12px] text-indigo-100/90 whitespace-pre-wrap break-words leading-relaxed max-h-96 overflow-auto mt-2">
            {translation}
          </pre>
        ) : (
          <pre className="text-[11px] text-violet-200/80 font-mono whitespace-pre-wrap break-words leading-relaxed max-h-96 overflow-auto mt-2">
            {displayText}
          </pre>
        )}
      </div>
    </div>
  );
}

function SystemMessageSummary({ event }: { event: TimelineEvent }) {
  const { preview, content_chars, message_id, slot_id, role } = event.payload || {};
  const fullMsg = useFullMessage(message_id, true);
  const displayText = fullMsg?.content ?? preview;

  return (
    <div className="space-y-3">
      <div className="flex items-center gap-2">
        <span className="text-[10px] uppercase tracking-wider text-neutral-500 font-medium">Daemon</span>
        {slot_id && <span className="text-[10px] text-teal-400/70 bg-teal-500/10 px-1.5 py-0.5 rounded font-mono">{slot_id}</span>}
        {role && <span className="text-[10px] text-neutral-600 bg-neutral-900 px-1.5 py-0.5 rounded">{role}</span>}
        <span className="text-[10px] text-neutral-600 bg-neutral-900 px-1.5 py-0.5 rounded font-mono">{content_chars} chars</span>
      </div>
      {displayText && (
        <div className="p-3 rounded-lg text-sm leading-relaxed bg-slate-500/10 border border-slate-500/20 max-h-[600px] overflow-auto">
          <MarkdownContent content={displayText} />
        </div>
      )}
    </div>
  );
}

function SlotStateSummary({ event }: { event: TimelineEvent }) {
  const { slot_id, prev_state, new_state } = event.payload || {};
  return (
    <div className="flex flex-col items-center justify-center py-6 bg-neutral-900/50 rounded-lg border border-neutral-800">
      <div className="flex items-center gap-1.5 text-neutral-400 mb-4 bg-neutral-800 px-2 py-1 rounded text-xs font-mono">
        <Settings2 className="w-3 h-3" /> {slot_id}
      </div>
      <div className="flex items-center gap-4">
        <SlotBadge state={prev_state || '?'} />
        <div className="flex flex-col items-center text-neutral-500">
          <ArrowRight className="w-5 h-5" />
        </div>
        <SlotBadge state={new_state || '?'} />
      </div>
    </div>
  );
}

function SlotBadge({ state }: { state: string }) {
  const isActive = state === 'Thinking' || state === 'Working' || state === 'Running';
  return (
    <div className={cn(
      'px-4 py-2 rounded-full text-xs font-medium border',
      isActive ? 'bg-amber-500/20 border-amber-500/30 text-amber-300' : 'bg-slate-500/20 border-slate-500/30 text-slate-300',
    )}>
      {state}
    </div>
  );
}

function TaskSummary({ event }: { event: TimelineEvent }) {
  const p = event.payload || {};
  const isUpdate = event.event_type === 'board_task_updated';
  return (
    <div className="border border-blue-500/20 bg-blue-950/10 rounded-lg p-4 space-y-2">
      <div className="flex items-center gap-2">
        <Activity className="w-4 h-4 text-blue-400" />
        <span className="text-xs font-medium text-blue-300">{isUpdate ? 'Board Task Updated' : 'Task Lifecycle'}</span>
        {p.action && (
          <span className={cn(
            'text-[10px] px-1.5 py-0.5 rounded font-medium',
            p.action === 'completed' ? 'bg-green-500/20 text-green-400' :
            p.action === 'created' ? 'bg-blue-500/20 text-blue-400' : 'bg-neutral-800 text-neutral-400',
          )}>
            {p.action}
          </span>
        )}
        {p.status && (
          <span className={cn(
            'text-[10px] px-1.5 py-0.5 rounded font-medium',
            p.status === 'done' ? 'bg-green-500/20 text-green-400' : 'bg-neutral-800 text-neutral-400',
          )}>
            {p.status}
          </span>
        )}
      </div>
      {p.title && <p className="text-sm text-neutral-200">{p.title}</p>}
      {p.task_id && <p className="text-[10px] text-neutral-500 font-mono">{p.task_id}</p>}
    </div>
  );
}

function LlmRequestSummary({ event }: { event: TimelineEvent }) {
  const p = event.payload || {};
  const isStarted = event.event_type.endsWith('_started');
  // Unified cli_request_* events carry engine in payload; legacy codex_* events are always codex
  const engine = p.engine || (event.event_type.startsWith('codex_') ? 'codex' : 'gemini');
  const isCodex = engine === 'codex';
  const engineLabel = engine === 'codex' ? 'GPT' : engine === 'gemini' ? 'Gemini' : engine;
  const accent = isCodex ? 'sky' : 'purple';
  const [imageExpanded, setImageExpanded] = useState(false);

  return (
    <div className={cn(
      'border rounded-lg overflow-hidden',
      `border-${accent}-500/20 bg-${accent}-950/10`,
    )} style={{
      borderColor: isCodex ? 'rgba(56,189,248,0.2)' : 'rgba(168,85,247,0.2)',
      backgroundColor: isCodex ? 'rgba(8,47,73,0.1)' : 'rgba(59,7,100,0.1)',
    }}>
      <div className="flex items-center gap-2 p-3 pb-0">
        <Cpu className={cn('w-4 h-4', isCodex ? 'text-sky-400' : 'text-purple-400')} />
        <span className={cn('text-xs font-medium', isCodex ? 'text-sky-300' : 'text-purple-300')}>
          {engineLabel} — {isStarted ? 'Request Sent' : 'Response Received'}
        </span>
        {p.error && <span className="text-[10px] px-1.5 py-0.5 rounded bg-red-500/20 text-red-400">Error</span>}
      </div>

      <div className="flex gap-3 p-3">
        {/* Left: content (prompt/response + image) */}
        <div className="flex-1 min-w-0 space-y-3">
          {p.image_hash && (
            <div>
              <button
                onClick={() => setImageExpanded(v => !v)}
                className="flex items-center gap-1 text-[9px] text-neutral-500 uppercase tracking-wider mb-1 hover:text-neutral-300 transition-colors"
              >
                {imageExpanded ? <ChevronDown className="w-3 h-3" /> : <ChevronRight className="w-3 h-3" />}
                Source Image
              </button>
              {imageExpanded ? (
                <img
                  src={`/api/images?hash=${p.image_hash}`}
                  alt="Vision source"
                  className="max-w-full max-h-[500px] rounded-lg border border-neutral-700 object-contain"
                />
              ) : (
                <img
                  src={`/api/images?hash=${p.image_hash}`}
                  alt="Vision source"
                  className="w-24 h-16 rounded border border-neutral-700 object-cover cursor-pointer hover:opacity-80 transition-opacity"
                  onClick={() => setImageExpanded(true)}
                />
              )}
            </div>
          )}
          {p.request_id && <GeminiContentPanel requestId={p.request_id} isResponse={!isStarted} />}
        </div>

        {/* Right: stat cards */}
        <div className="w-40 shrink-0 space-y-1.5">
          <MiniStat label="Model" value={p.model || '-'} />
          <MiniStat label="Caller" value={p.caller || '-'} />
          {!isStarted && <MiniStat label="Duration" value={p.duration_ms ? `${(p.duration_ms / 1000).toFixed(1)}s` : '-'} />}
          <MiniStat label="Prompt" value={`${p.prompt_chars || 0} chars`} />
          {!isStarted && <MiniStat label="Response" value={`${p.response_chars || 0} chars`} />}
          {!isStarted && p.status && <MiniStat label="Status" value={p.status} />}
          {p.has_image && !p.image_hash && <MiniStat label="Image" value="Yes" />}
          {p.output_tokens != null && <MiniStat label="Out Tokens" value={`${p.output_tokens}`} />}
        </div>
      </div>
    </div>
  );
}

function GeminiContentPanel({ requestId, isResponse }: { requestId: string; isResponse?: boolean }) {
  const [content, setContent] = useState<{ prompt_text?: string; response_text?: string } | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [promptOpen, setPromptOpen] = useState(!isResponse);
  const [responseOpen, setResponseOpen] = useState(true);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    fetch(`/api/system/gemini-content?request_id=${encodeURIComponent(requestId)}`)
      .then(r => r.json())
      .then(data => {
        if (!cancelled) {
          if (data.error) setError(data.error);
          else setContent(data);
        }
      })
      .catch(e => { if (!cancelled) setError(String(e)); })
      .finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, [requestId]);

  if (loading) return <div className="text-[11px] text-neutral-500 animate-pulse">Loading content...</div>;
  if (error) return <div className="text-[11px] text-red-400">Failed: {error}</div>;
  if (!content) return null;

  return (
    <div className="space-y-3">
      {content.prompt_text && (
        <div>
          <button
            onClick={() => setPromptOpen(v => !v)}
            className="flex items-center gap-1 text-[9px] text-neutral-500 uppercase tracking-wider mb-1 hover:text-neutral-300 transition-colors"
          >
            {promptOpen ? <ChevronDown className="w-3 h-3" /> : <ChevronRight className="w-3 h-3" />}
            Prompt
          </button>
          {promptOpen && (
            <pre className="text-[11px] text-purple-300/80 font-mono bg-neutral-900 rounded p-3 overflow-auto max-h-60 whitespace-pre-wrap break-words leading-relaxed">
              {content.prompt_text}
            </pre>
          )}
        </div>
      )}
      {content.response_text && (
        <div>
          <button
            onClick={() => setResponseOpen(v => !v)}
            className="flex items-center gap-1 text-[9px] text-neutral-500 uppercase tracking-wider mb-1 hover:text-neutral-300 transition-colors"
          >
            {responseOpen ? <ChevronDown className="w-3 h-3" /> : <ChevronRight className="w-3 h-3" />}
            Response
          </button>
          {responseOpen && (
            <pre className="text-[11px] text-emerald-300/80 font-mono bg-neutral-900 rounded p-3 overflow-auto max-h-60 whitespace-pre-wrap break-words leading-relaxed">
              {content.response_text}
            </pre>
          )}
        </div>
      )}
    </div>
  );
}

function GitCommitSummary({ event }: { event: TimelineEvent }) {
  const { short_hash, hash, message, author, repo, committed_at } = event.payload || {};
  return (
    <div className="border border-green-500/20 bg-green-950/10 rounded-lg p-4">
      <div className="flex items-start justify-between mb-3">
        <div className="flex items-center gap-2 text-green-400 font-mono text-sm bg-green-500/10 px-2 py-1 rounded">
          <GitCommit className="w-4 h-4" />
          {short_hash}
        </div>
        {repo && <span className="text-[10px] text-neutral-500 uppercase bg-neutral-900 px-2 py-1 rounded">{repo}</span>}
      </div>
      <p className="text-sm text-neutral-200 font-medium mb-4 leading-relaxed">{message}</p>
      <div className="flex items-center gap-4 text-xs text-neutral-400">
        {author && <div className="flex items-center gap-1.5"><User className="w-3.5 h-3.5" />{author}</div>}
        {committed_at && <div className="flex items-center gap-1.5"><Clock className="w-3.5 h-3.5" />{formatBeijing(committed_at)}</div>}
      </div>
      {hash && <p className="text-[10px] text-neutral-600 font-mono mt-2 select-all">{hash}</p>}
    </div>
  );
}

function MemoryPhaseSummary({ event }: { event: TimelineEvent }) {
  const p = event.payload || {};
  return (
    <div className="flex flex-col items-center justify-center py-6 bg-neutral-900/50 rounded-lg border border-neutral-800">
      <Brain className="w-5 h-5 text-indigo-400 mb-2" />
      <span className="text-[10px] text-neutral-500 uppercase tracking-wider mb-3">Memory Phase</span>
      <div className="flex items-center gap-4">
        <SlotBadge state={p.prev_phase || p.from || '?'} />
        <ArrowRight className="w-5 h-5 text-neutral-500" />
        <SlotBadge state={p.new_phase || p.to || '?'} />
      </div>
    </div>
  );
}

function DecisionSummary({ event }: { event: TimelineEvent }) {
  const p = event.payload || {};
  return (
    <div className="border border-amber-500/20 bg-amber-950/10 rounded-lg p-4 space-y-2">
      <div className="flex items-center gap-2">
        <Zap className="w-4 h-4 text-amber-400" />
        <span className="text-xs font-medium text-amber-300">Decision Made</span>
        {p.tier && <span className="text-[10px] bg-amber-500/20 text-amber-400 px-1.5 py-0.5 rounded">{p.tier}</span>}
      </div>
      {p.question && <p className="text-sm text-neutral-200 leading-relaxed">{p.question}</p>}
      {p.answer && <p className="text-xs text-neutral-400 leading-relaxed">{p.answer}</p>}
    </div>
  );
}

function InsightSummary({ event }: { event: TimelineEvent }) {
  const p = event.payload || {};
  return (
    <div className="border border-emerald-500/30 bg-emerald-950/20 rounded-lg p-4 space-y-2">
      <div className="flex items-center gap-1.5">
        <Sparkles className="w-4 h-4 text-emerald-400" />
        <span className="text-xs font-medium text-emerald-300">Insight</span>
      </div>
      <p className="text-sm text-emerald-100 leading-relaxed">{p.title || eventSummary(event)}</p>
      {p.body && <p className="text-xs text-emerald-300/70 leading-relaxed">{p.body}</p>}
    </div>
  );
}

function QuestionSummary({ event }: { event: TimelineEvent }) {
  const p = event.payload || {};
  const isResolved = event.event_type === 'question_resolved';
  return (
    <div className="border border-amber-500/20 bg-amber-950/10 rounded-lg p-4 space-y-2">
      <div className="flex items-center gap-2">
        <MessageSquare className="w-4 h-4 text-amber-400" />
        <span className="text-xs font-medium text-amber-300">{isResolved ? 'Question Resolved' : 'Question Created'}</span>
      </div>
      {p.question && <p className="text-sm text-neutral-200 leading-relaxed">{p.question}</p>}
      {isResolved && p.answer && <p className="text-xs text-green-400 leading-relaxed mt-1">{p.answer}</p>}
      {p.question_id && <p className="text-[10px] text-neutral-500 font-mono">{p.question_id}</p>}
    </div>
  );
}

function DefaultSummary({ event }: { event: TimelineEvent }) {
  return (
    <div className="space-y-2">
      <p className="text-xs text-neutral-300">{eventSummary(event)}</p>
      {event.summary && <p className="text-xs text-neutral-500 italic">{event.summary}</p>}
    </div>
  );
}
