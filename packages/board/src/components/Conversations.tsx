'use client';

import { useState, useEffect, useCallback, useMemo } from 'react';
import { Search, RefreshCw, MessageSquare, User, Bot, Wrench, ArrowLeft, ChevronRight, ChevronDown, GitBranch, Terminal, Brain, Timer, Layers, Zap, Tag, Sparkles, Server } from 'lucide-react';
import { cn } from '@/lib/utils';
import { Badge } from '@/components/ui/badge';

interface Conversation {
  id: string;
  project: string | null;
  slotId: string | null;
  source: string;
  model: string | null;
  gitBranch: string | null;
  jsonlPath: string | null;
  parentSessionId: string | null;
  taskId: string | null;
  messageCount: number;
  startedAt: string;
  endedAt: string | null;
  status: string;
  conversationType: string;
  chatType: string | null;
  llmSummary: string | null;
}

interface ConversationMessage {
  id: number;
  sessionId: string;
  role: string;
  roleDisplay: string | null;
  content: string;
  rawContent: string | null;
  messageUuid: string | null;
  model: string | null;
  timestamp: string;
  metadata: string | null;
  toolName: string | null;
  seq: number | null;
}

interface ConversationEvent {
  id: number;
  sessionId: string;
  eventType: string;
  content: string | null;
  timestamp: string;
}

function timeAgo(dateStr: string): string {
  const diff = Date.now() - new Date(dateStr).getTime();
  const mins = Math.floor(diff / 60000);
  if (mins < 1) return '刚刚';
  if (mins < 60) return `${mins}分前`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}时前`;
  const days = Math.floor(hours / 24);
  if (days < 30) return `${days}天前`;
  return new Date(dateStr).toLocaleDateString('zh-CN');
}

function formatTime(dateStr: string): string {
  const d = new Date(dateStr);
  return d.toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit', second: '2-digit' });
}

function formatDate(dateStr: string): string {
  const d = new Date(dateStr);
  return d.toLocaleDateString('zh-CN', { month: 'short', day: 'numeric' });
}

const ROLE_CONFIG: Record<string, { icon: typeof User; color: string; label: string }> = {
  user: { icon: User, color: 'text-blue-400', label: '用户' },
  system: { icon: Terminal, color: 'text-orange-400', label: '系统指令' },
  assistant: { icon: Bot, color: 'text-green-400', label: 'AI' },
  tool_use: { icon: Wrench, color: 'text-amber-400', label: '工具调用' },
  tool_result: { icon: Wrench, color: 'text-neutral-500', label: '工具结果' },
  thinking: { icon: Brain, color: 'text-purple-400', label: '思考' },
  agent_user: { icon: User, color: 'text-cyan-400', label: 'Agent 用户' },
  agent_assistant: { icon: Bot, color: 'text-teal-400', label: 'Agent AI' },
};

function ImageBlock({ jsonlPath, messageUuid, imageIndex }: {
  jsonlPath: string;
  messageUuid: string;
  imageIndex: number;
}) {
  const [expanded, setExpanded] = useState(false);
  const src = `/api/conversation-image?path=${encodeURIComponent(jsonlPath)}&uuid=${encodeURIComponent(messageUuid)}&index=${imageIndex}`;

  return (
    <div className="my-2">
      {/* eslint-disable-next-line @next/next/no-img-element */}
      <img
        src={src}
        alt="用户截图"
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

/** Render message content with inline images from rawContent when available */
function MessageContent({ msg, jsonlPath }: { msg: ConversationMessage; jsonlPath?: string | null }) {
  const blocks = useMemo(() => {
    if (!msg.rawContent) return null;
    try {
      const raw = JSON.parse(msg.rawContent);
      if (!Array.isArray(raw)) return null;
      // Only use rich rendering if there are image blocks
      if (!raw.some((b: Record<string, unknown>) => b.type === 'image')) return null;
      return raw as Array<Record<string, unknown>>;
    } catch {
      return null;
    }
  }, [msg.rawContent]);

  // Rich rendering: interleave text and images from rawContent
  if (blocks && jsonlPath && msg.messageUuid) {
    let imageIdx = 0;
    return (
      <>
        {blocks.map((block, i) => {
          if (block.type === 'text') {
            return <span key={i}>{block.text as string}</span>;
          }
          if (block.type === 'image') {
            const idx = imageIdx++;
            return (
              <ImageBlock
                key={i}
                jsonlPath={jsonlPath}
                messageUuid={msg.messageUuid!}
                imageIndex={idx}
              />
            );
          }
          if (block.type === 'tool_use') {
            return <span key={i} className="text-amber-400/70">[Tool: {block.name as string}]</span>;
          }
          return null;
        })}
      </>
    );
  }

  // Fallback: plain text
  return <>{msg.content}</>;
}

const LABEL_STYLES: Record<string, { text: string; bg: string; short: string }> = {
  has_tool_use: { text: 'text-amber-300', bg: 'bg-amber-500/10', short: 'tool_use' },
  has_tool_result: { text: 'text-neutral-400', bg: 'bg-neutral-500/10', short: 'tool_result' },
  has_code_change: { text: 'text-green-300', bg: 'bg-green-500/10', short: 'code_change' },
  has_mcp_call: { text: 'text-cyan-300', bg: 'bg-cyan-500/10', short: 'mcp' },
  has_image: { text: 'text-pink-300', bg: 'bg-pink-500/10', short: 'image' },
  role_mapped: { text: 'text-purple-300', bg: 'bg-purple-500/10', short: 'role' },
  gemini_chat: { text: 'text-blue-300', bg: 'bg-blue-500/10', short: 'gemini' },
  gemini_channel: { text: 'text-indigo-300', bg: 'bg-indigo-500/10', short: 'channel' },
};

function LabelBadges({ labels }: { labels: [string, string][] }) {
  if (!labels || labels.length === 0) return null;
  return (
    <div className="flex items-center gap-1 flex-wrap">
      {labels.map(([label, value]) => {
        const style = LABEL_STYLES[label] || { text: 'text-neutral-400', bg: 'bg-neutral-500/10', short: label };
        const display = value === 'true' ? style.short : `${style.short}:${value}`;
        return (
          <span
            key={label}
            className={cn('px-1.5 py-0.5 text-[9px] font-mono rounded', style.text, style.bg)}
          >
            {display}
          </span>
        );
      })}
    </div>
  );
}

function MessageBubble({ msg, jsonlPath, labels }: { msg: ConversationMessage; jsonlPath?: string | null; labels?: [string, string][] }) {
  const [expanded, setExpanded] = useState(false);
  const config = ROLE_CONFIG[msg.role] || ROLE_CONFIG.assistant;
  const Icon = config.icon;

  // Check if this message has images (use rich rendering for those)
  const hasImages = msg.rawContent?.includes('"type":"image"') || msg.content.includes('[图片]');

  // Role display with emoji
  const roleEmoji: Record<string, string> = {
    user: '👤', assistant: '🤖', tool_use: '🔧', tool_result: '🔧',
    thinking: '🧠', system: '⚙️', agent_user: '👤', agent_assistant: '🤖',
    compact_summary: '📋',
  };

  return (
    <div className="border-b border-neutral-800/30 py-3">
      {/* Header: role + msg ID + timestamp + tool */}
      <div className="flex items-center gap-2 mb-1.5 flex-wrap">
        <Icon className={cn('w-3.5 h-3.5', config.color)} />
        <span className={cn('text-sm font-semibold', config.color)}>
          {roleEmoji[msg.role] || '📄'} {msg.roleDisplay || config.label} (msg {msg.id})
        </span>
        {msg.toolName && (
          <span className="text-xs font-mono px-1.5 py-0.5 rounded bg-neutral-800 text-amber-300">
            {msg.toolName}
          </span>
        )}
        {labels && labels.length > 0 && <LabelBadges labels={labels} />}
      </div>

      {/* Timestamp line */}
      <div className="text-xs text-cyan-500/70 mb-2 font-mono">
        {msg.timestamp}
        {msg.model && <span className="ml-3 text-neutral-600">{msg.model}</span>}
        {msg.seq != null && <span className="ml-3 text-neutral-700">seq:{msg.seq}</span>}
      </div>

      {/* Content — raw, no hiding */}
      <div className={cn(
        'text-sm leading-relaxed whitespace-pre-wrap break-words',
        msg.role === 'user' ? 'text-neutral-200' : 'text-neutral-400',
        msg.role === 'thinking' && 'text-purple-300/70',
        (msg.role === 'tool_result') && 'font-mono text-xs',
      )}>
        {hasImages ? (
          <MessageContent msg={msg} jsonlPath={jsonlPath} />
        ) : msg.content.length > 2000 ? (
          <>
            <div>{expanded ? msg.content : msg.content.slice(0, 2000)}</div>
            {!expanded && (
              <button
                onClick={() => setExpanded(true)}
                className="text-[11px] text-cyan-500 hover:text-cyan-400 mt-1"
              >
                ▼ 展开全部 ({msg.content.length.toLocaleString()} chars)
              </button>
            )}
            {expanded && (
              <button
                onClick={() => setExpanded(false)}
                className="text-[11px] text-cyan-500 hover:text-cyan-400 mt-1"
              >
                ▲ 收起
              </button>
            )}
          </>
        ) : (
          msg.content
        )}
      </div>
    </div>
  );
}

/** Render a system event inline in the message timeline */
function EventBubble({ event }: { event: ConversationEvent }) {
  const { icon: Icon, color, label } = (() => {
    const t = event.eventType;
    if (t === 'turn_duration') return { icon: Timer, color: 'text-neutral-500', label: 'Turn' };
    if (t === 'compact_boundary') return { icon: Layers, color: 'text-yellow-500', label: 'Context 压缩' };
    if (t.startsWith('queue:')) return { icon: Zap, color: 'text-neutral-600', label: t.replace('queue:', 'Queue: ') };
    if (t === 'hook_progress') return { icon: Zap, color: 'text-neutral-600', label: 'Hook' };
    return { icon: Terminal, color: 'text-neutral-600', label: t };
  })();

  return (
    <div className="flex items-center gap-2 py-0.5 opacity-50 hover:opacity-80 transition-opacity">
      <Icon className={cn('w-3 h-3 flex-shrink-0', color)} />
      <span className={cn('text-[10px] font-mono', color)}>{label}</span>
      {event.content && (
        <span className="text-[10px] text-neutral-600 truncate">{event.content}</span>
      )}
      <span className="text-[10px] text-neutral-700 ml-auto flex-shrink-0">{formatTime(event.timestamp)}</span>
    </div>
  );
}

function ConversationListItem({
  conv,
  active,
  onClick,
  subagentCount,
  expanded,
  onToggleExpand,
  isSubagent,
}: {
  conv: Conversation;
  active: boolean;
  onClick: () => void;
  subagentCount?: number;
  expanded?: boolean;
  onToggleExpand?: () => void;
  isSubagent?: boolean;
}) {
  return (
    <div className={cn(isSubagent && 'ml-4 border-l border-neutral-800/50 pl-1')}>
      <button
        onClick={onClick}
        className={cn(
          'w-full text-left p-3 rounded-lg border transition-colors',
          active
            ? 'bg-neutral-800/50 border-orange-500/30'
            : 'border-neutral-800/50 hover:border-neutral-700',
          isSubagent && 'py-2',
        )}
      >
        <div className="flex items-center justify-between mb-1">
          <div className="flex items-center gap-2 min-w-0">
            {isSubagent && (
              <GitBranch className="w-3 h-3 text-neutral-600 flex-shrink-0" />
            )}
            {conv.project && (
              <span className="text-[11px] font-mono text-orange-400/80 bg-orange-500/10 px-1.5 py-0.5 rounded truncate max-w-[140px]">
                {conv.project.split('/').pop()}
              </span>
            )}
            {conv.slotId && (
              <span className="text-[10px] font-mono text-neutral-600 truncate">
                {conv.slotId}
              </span>
            )}
          </div>
          <div className="flex items-center gap-1.5 flex-shrink-0">
            {subagentCount && subagentCount > 0 ? (
              <button
                onClick={(e) => {
                  e.stopPropagation();
                  onToggleExpand?.();
                }}
                className="flex items-center gap-0.5 text-[10px] text-neutral-500 hover:text-neutral-300 transition-colors px-1 py-0.5 rounded hover:bg-neutral-800"
                title={expanded ? '收起子任务' : '展开子任务'}
              >
                {expanded ? <ChevronDown className="w-3 h-3" /> : <ChevronRight className="w-3 h-3" />}
                <span>{subagentCount} 子任务</span>
              </button>
            ) : null}
            <Badge
              variant="outline"
              className={cn(
                'text-[10px] border-neutral-800',
                conv.status === 'active' ? 'text-green-500'
                  : conv.status === 'compacted' ? 'text-yellow-600'
                  : 'text-neutral-600',
              )}
            >
              {conv.status === 'active' ? '进行中' : conv.status === 'compacted' ? '已压缩' : '已完成'}
            </Badge>
          </div>
        </div>

        {conv.llmSummary && (
          <p className="text-[11px] text-neutral-500 mt-1 line-clamp-2 leading-relaxed">
            {conv.llmSummary}
          </p>
        )}

        <div className="flex items-center justify-between text-[11px] text-neutral-500 mt-1">
          <div className="flex items-center gap-2 min-w-0">
            {conv.messageCount > 0 && <span>{conv.messageCount} 条消息</span>}
            {conv.taskId && (
              <span className="font-mono text-blue-400/60 truncate max-w-[80px]" title={conv.taskId}>
                {conv.taskId.slice(0, 8)}
              </span>
            )}
            {conv.slotId && (
              <span className="font-mono text-cyan-500/60">{conv.slotId}</span>
            )}
            {conv.source === 'pty_jsonl' && (
              <span className="text-[9px] text-purple-500/50">PTY</span>
            )}
            {conv.model && (
              <span className="font-mono text-neutral-600 truncate max-w-[100px]">{conv.model}</span>
            )}
          </div>
          <div className="flex items-center gap-2 flex-shrink-0">
            {conv.endedAt && (
              <span className="text-[10px] text-neutral-600" title="持续时间">
                {(() => {
                  const ms = new Date(conv.endedAt).getTime() - new Date(conv.startedAt).getTime();
                  if (ms < 60000) return `${Math.round(ms / 1000)}s`;
                  if (ms < 3600000) return `${Math.round(ms / 60000)}m`;
                  return `${(ms / 3600000).toFixed(1)}h`;
                })()}
              </span>
            )}
            <span>{timeAgo(conv.startedAt)}</span>
          </div>
        </div>

        {conv.gitBranch && (
          <div className="text-[10px] text-neutral-600 font-mono mt-1 truncate">
            {conv.gitBranch}
          </div>
        )}
      </button>
    </div>
  );
}

function GeminiListItem({ conv, active, onClick }: { conv: Conversation; active: boolean; onClick: () => void }) {
  // Derive display label: taskId for router_chat, project name for gemini_cli
  const label = conv.taskId
    ? conv.taskId.slice(0, 8)
    : conv.project
      ? conv.project.split('/').filter(Boolean).pop() || 'gemini'
      : 'gemini';
  const sourceTag = conv.source === 'gemini_cli' ? 'CLI' : 'Chat';
  return (
    <button
      onClick={onClick}
      className={cn(
        'w-full text-left px-3 py-2 rounded-md border transition-colors',
        active
          ? 'bg-neutral-800/50 border-indigo-500/30'
          : 'border-neutral-800/30 hover:border-neutral-700',
      )}
    >
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2 min-w-0">
          <Sparkles className="w-3 h-3 text-indigo-400 flex-shrink-0" />
          <span className="text-[11px] font-mono text-indigo-300/80 truncate max-w-[120px]">
            {label}
          </span>
          <span className="text-[10px] font-mono text-neutral-600">{conv.model || 'gemini'}</span>
          <span className="text-[9px] px-1 rounded bg-neutral-800 text-neutral-500">{sourceTag}</span>
        </div>
        <div className="flex items-center gap-2 flex-shrink-0">
          {conv.messageCount > 0 && (
            <span className="text-[10px] text-neutral-600">{conv.messageCount} 条</span>
          )}
          <span className={cn('text-[10px]', conv.status === 'active' ? 'text-green-500/70' : 'text-neutral-600')}>
            {conv.status === 'active' ? '进行中' : '已完成'}
          </span>
          <span className="text-[10px] text-neutral-600">{timeAgo(conv.startedAt)}</span>
        </div>
      </div>
    </button>
  );
}

export function Conversations() {
  const [conversations, setConversations] = useState<Conversation[]>([]);
  const [messages, setMessages] = useState<ConversationMessage[]>([]);
  const [events, setEvents] = useState<ConversationEvent[]>([]);
  const [labelsMap, setLabelsMap] = useState<Record<string, [string, string][]>>({});
  const [showLabels, setShowLabels] = useState(false);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [jsonlPath, setJsonlPath] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadingMessages, setLoadingMessages] = useState(false);
  const [search, setSearch] = useState('');
  const [searchResults, setSearchResults] = useState<ConversationMessage[] | null>(null);
  const [statusFilter, setStatusFilter] = useState<string | null>(null);
  const [viewMode, setViewMode] = useState<'conversations' | 'workers' | 'gemini'>('conversations');
  const [showList, setShowList] = useState(true); // mobile: toggle list/detail
  const [expandedParents, setExpandedParents] = useState<Set<string>>(new Set());
  const [hasMore, setHasMore] = useState(false); // whether more messages exist beyond loaded window
  const [loadingMore, setLoadingMore] = useState(false);
  const [totalMessageCount, setTotalMessageCount] = useState(0);

  const fetchConversations = useCallback(async () => {
    setLoading(true);
    try {
      const params = new URLSearchParams();
      if (statusFilter) params.set('status', statusFilter);
      params.set('limit', '300');
      const res = await fetch(`/api/conversations?${params}`);
      if (res.ok) {
        const data = await res.json();
        const list: Conversation[] = Array.isArray(data) ? data : [];
        // Sort: active first, then by most recent
        list.sort((a, b) => {
          if (a.status === 'active' && b.status !== 'active') return -1;
          if (a.status !== 'active' && b.status === 'active') return 1;
          return new Date(b.startedAt).getTime() - new Date(a.startedAt).getTime();
        });
        setConversations(list);
      }
    } catch {
      // silent
    }
    setLoading(false);
  }, [statusFilter]);

  const PAGE_SIZE = 500;

  const fetchMessages = useCallback(async (sessionId: string, withLabels?: boolean) => {
    setLoadingMessages(true);
    setSearchResults(null);
    try {
      const labelsParam = withLabels ? '&labels=1' : '';
      // Load from beginning (sinceId=0) instead of tail
      const res = await fetch(`/api/conversations?sessionId=${encodeURIComponent(sessionId)}&sinceId=0&tail=${PAGE_SIZE}${labelsParam}`);
      if (res.ok) {
        const data = await res.json();
        const msgs = data.messages || [];
        setMessages(msgs);
        setEvents(data.events || []);
        setJsonlPath(data.conversation?.jsonlPath || null);
        setLabelsMap(data.labels || {});
        const total = data.conversation?.messageCount || 0;
        setTotalMessageCount(total);
        setHasMore(msgs.length >= PAGE_SIZE);
      }
    } catch {
      setMessages([]);
      setEvents([]);
      setJsonlPath(null);
      setLabelsMap({});
      setHasMore(false);
    }
    setLoadingMessages(false);
  }, []);

  const loadMoreMessages = useCallback(async () => {
    if (!selectedId || loadingMore || !hasMore || messages.length === 0) return;
    setLoadingMore(true);
    try {
      const lastId = messages[messages.length - 1].id;
      const res = await fetch(`/api/conversations?sessionId=${encodeURIComponent(selectedId)}&sinceId=${lastId}&tail=${PAGE_SIZE}`);
      if (res.ok) {
        const data = await res.json();
        const newMsgs: ConversationMessage[] = data.messages || [];
        if (newMsgs.length > 0) {
          setMessages(prev => [...prev, ...newMsgs]);
        }
        setHasMore(newMsgs.length >= PAGE_SIZE);
      }
    } catch { /* ignore */ }
    setLoadingMore(false);
  }, [selectedId, loadingMore, hasMore, messages]);

  const handleSearch = useCallback(async () => {
    if (!search.trim()) {
      setSearchResults(null);
      return;
    }
    setLoading(true);
    try {
      const res = await fetch(`/api/conversations?search=${encodeURIComponent(search)}&limit=50`);
      if (res.ok) {
        const data = await res.json();
        setSearchResults(data.results || []);
      }
    } catch {
      setSearchResults([]);
    }
    setLoading(false);
  }, [search]);

  useEffect(() => {
    fetchConversations();
  }, [fetchConversations]);

  const selectConversation = useCallback((id: string) => {
    setSelectedId(id);
    setShowList(false);
    fetchMessages(id, showLabels);
  }, [fetchMessages, showLabels]);

  const selectedConv = useMemo(
    () => conversations.find((c) => c.id === selectedId),
    [conversations, selectedId],
  );

  // Merge messages and important events into a unified timeline grouped by date
  type TimelineItem = { type: 'message'; data: ConversationMessage } | { type: 'event'; data: ConversationEvent };
  const groupedTimeline = useMemo(() => {
    // Only show events within the time range of loaded messages (tail=500 means
    // earlier events would appear orphaned without their corresponding messages)
    const earliestMsg = messages.length > 0
      ? new Date(messages[0].timestamp).getTime()
      : Infinity;

    const importantEvents = events.filter((e) => {
      const t = e.eventType;
      if (!(t === 'turn_duration' || t === 'compact_boundary' || t.startsWith('queue:'))) return false;
      return new Date(e.timestamp).getTime() >= earliestMsg;
    });

    // Merge into timeline sorted by timestamp
    const timeline: TimelineItem[] = [
      ...messages.map((m) => ({ type: 'message' as const, data: m })),
      ...importantEvents.map((e) => ({ type: 'event' as const, data: e })),
    ];
    timeline.sort((a, b) => new Date(a.data.timestamp).getTime() - new Date(b.data.timestamp).getTime());

    // Group by date
    const groups: { date: string; items: TimelineItem[] }[] = [];
    let currentDate = '';
    for (const item of timeline) {
      const date = formatDate(item.data.timestamp);
      if (date !== currentDate) {
        currentDate = date;
        groups.push({ date, items: [item] });
      } else {
        groups[groups.length - 1].items.push(item);
      }
    }
    return groups;
  }, [messages, events]);

  const isGeminiSource = useCallback((source: string) => {
    return source === 'router_chat' || source === 'gemini_cli';
  }, []);

  const filterByTab = useCallback((c: Conversation, tab: typeof viewMode) => {
    if (tab === 'conversations') {
      return c.conversationType === 'user' && !isGeminiSource(c.source);
    }
    if (tab === 'gemini') {
      return isGeminiSource(c.source);
    }
    // workers: catch-all for non-user non-gemini
    return c.conversationType !== 'user' && !isGeminiSource(c.source);
  }, [isGeminiSource]);

  const counts = useMemo(() => {
    const filtered = conversations.filter((c) => filterByTab(c, viewMode));
    const active = filtered.filter((c) => c.status === 'active').length;
    const completed = filtered.filter((c) => c.status === 'completed').length;
    const compacted = filtered.filter((c) => c.status === 'compacted').length;
    return { active, completed, compacted, total: filtered.length };
  }, [conversations, viewMode, filterByTab]);

  const tabCounts = useMemo(() => ({
    conversations: conversations.filter((c) => filterByTab(c, 'conversations')).length,
    workers: conversations.filter((c) => filterByTab(c, 'workers')).length,
    gemini: conversations.filter((c) => filterByTab(c, 'gemini')).length,
  }), [conversations, filterByTab]);

  // Group: separate subagents and compacted sessions from main list, filter by viewMode
  const { mainList, subagentMap } = useMemo(() => {
    const map = new Map<string, Conversation[]>();
    const main: Conversation[] = [];
    for (const conv of conversations) {
      if (conv.parentSessionId) {
        const list = map.get(conv.parentSessionId) || [];
        list.push(conv);
        map.set(conv.parentSessionId, list);
      } else if (conv.status === 'compacted') {
        continue;
      } else if (filterByTab(conv, viewMode)) {
        main.push(conv);
      }
    }
    return { mainList: main, subagentMap: map };
  }, [conversations, viewMode, filterByTab]);

  const toggleParentExpand = useCallback((sessionId: string) => {
    setExpandedParents((prev) => {
      const next = new Set(prev);
      if (next.has(sessionId)) {
        next.delete(sessionId);
      } else {
        next.add(sessionId);
      }
      return next;
    });
  }, []);

  return (
    <div className="flex-1 flex min-h-0 overflow-hidden">
      {/* Left: Conversation list */}
      <div className={cn(
        'w-80 flex-shrink-0 border-r border-neutral-800 flex flex-col',
        !showList && 'hidden md:flex',
      )}>
        {/* View mode tabs */}
        <div className="flex border-b border-neutral-800">
          <button
            onClick={() => setViewMode('conversations')}
            className={cn(
              'flex-1 py-2 text-xs font-medium transition-colors flex items-center justify-center gap-1.5',
              viewMode === 'conversations'
                ? 'text-neutral-200 border-b-2 border-blue-500'
                : 'text-neutral-500 hover:text-neutral-400',
            )}
          >
            <MessageSquare className="w-3 h-3" />
            对话 {tabCounts.conversations > 0 && <span className="text-neutral-600">{tabCounts.conversations}</span>}
          </button>
          <button
            onClick={() => setViewMode('workers')}
            className={cn(
              'flex-1 py-2 text-xs font-medium transition-colors flex items-center justify-center gap-1.5',
              viewMode === 'workers'
                ? 'text-neutral-200 border-b-2 border-orange-500'
                : 'text-neutral-500 hover:text-neutral-400',
            )}
          >
            <Server className="w-3 h-3" />
            工位 {tabCounts.workers > 0 && <span className="text-neutral-600">{tabCounts.workers}</span>}
          </button>
          <button
            onClick={() => setViewMode('gemini')}
            className={cn(
              'flex-1 py-2 text-xs font-medium transition-colors flex items-center justify-center gap-1.5',
              viewMode === 'gemini'
                ? 'text-neutral-200 border-b-2 border-indigo-500'
                : 'text-neutral-500 hover:text-neutral-400',
            )}
          >
            <Sparkles className="w-3 h-3" />
            Gemini {tabCounts.gemini > 0 && <span className="text-neutral-600">{tabCounts.gemini}</span>}
          </button>
        </div>

        {/* Search bar */}
        <div className="p-3 border-b border-neutral-800/50 space-y-2">
          <div className="relative">
            <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-neutral-500" />
            <input
              type="text"
              placeholder="搜索对话内容..."
              value={search}
              onChange={(e) => {
                setSearch(e.target.value);
                if (!e.target.value) setSearchResults(null);
              }}
              onKeyDown={(e) => e.key === 'Enter' && handleSearch()}
              className="w-full pl-9 pr-3 py-1.5 bg-neutral-900 border border-neutral-800 rounded-md text-xs text-neutral-300 placeholder:text-neutral-600 focus:outline-none focus:border-neutral-700"
            />
          </div>

          {/* Filters */}
          <div className="flex items-center gap-1.5">
            <button
              onClick={() => setStatusFilter(null)}
              className={cn(
                'px-2 py-0.5 text-[10px] rounded-full border transition-colors',
                !statusFilter
                  ? 'bg-neutral-800 text-white border-neutral-700'
                  : 'text-neutral-500 border-neutral-800 hover:text-neutral-300',
              )}
            >
              全部 {counts.total}
            </button>
            <button
              onClick={() => setStatusFilter('active')}
              className={cn(
                'px-2 py-0.5 text-[10px] rounded-full border transition-colors',
                statusFilter === 'active'
                  ? 'bg-green-500/10 text-green-400 border-green-500/30'
                  : 'text-neutral-500 border-neutral-800 hover:text-neutral-300',
              )}
            >
              进行中 {counts.active}
            </button>
            <button
              onClick={() => setStatusFilter('completed')}
              className={cn(
                'px-2 py-0.5 text-[10px] rounded-full border transition-colors',
                statusFilter === 'completed'
                  ? 'bg-neutral-700 text-neutral-300 border-neutral-600'
                  : 'text-neutral-500 border-neutral-800 hover:text-neutral-300',
              )}
            >
              已完成 {counts.completed}
            </button>
            <button
              onClick={fetchConversations}
              className="ml-auto p-1 rounded text-neutral-600 hover:text-neutral-400 transition-colors"
              title="刷新"
            >
              <RefreshCw className={cn('w-3 h-3', loading && 'animate-spin')} />
            </button>
          </div>
        </div>

        {/* Search results */}
        {searchResults !== null ? (
          <div className="flex-1 overflow-auto p-2 space-y-1">
            <div className="flex items-center justify-between px-1 mb-2">
              <span className="text-[11px] text-neutral-500">
                搜索到 {searchResults.length} 条消息
              </span>
              <button
                onClick={() => { setSearch(''); setSearchResults(null); }}
                className="text-[11px] text-neutral-600 hover:text-neutral-400"
              >
                清除
              </button>
            </div>
            {searchResults.map((msg) => (
              <button
                key={msg.id}
                onClick={() => selectConversation(msg.sessionId)}
                className="w-full text-left p-2 rounded-md border border-neutral-800/50 hover:border-neutral-700 transition-colors"
              >
                <div className="flex items-center gap-2 mb-0.5">
                  <span className={cn('text-[10px]', ROLE_CONFIG[msg.role]?.color || 'text-neutral-500')}>
                    {ROLE_CONFIG[msg.role]?.label || msg.role}
                  </span>
                  <span className="text-[10px] text-neutral-600">{timeAgo(msg.timestamp)}</span>
                </div>
                <p className="text-xs text-neutral-400 line-clamp-2">{msg.content}</p>
              </button>
            ))}
          </div>
        ) : (
          /* Conversation list */
          <div className="flex-1 overflow-auto p-2 space-y-1">
            {loading && conversations.length === 0 ? (
              <div className="text-center py-8 text-neutral-600 text-xs">加载中...</div>
            ) : mainList.length === 0 ? (
              <div className="text-center py-8 text-neutral-600 text-xs">
                {viewMode === 'conversations' ? '暂无对话记录' : viewMode === 'workers' ? '暂无工位会话' : '暂无 Gemini 审核记录'}
              </div>
            ) : viewMode === 'gemini' ? (
              <div className="space-y-0.5">
                {mainList.map((conv) => (
                  <GeminiListItem
                    key={conv.id}
                    conv={conv}
                    active={conv.id === selectedId}
                    onClick={() => selectConversation(conv.id)}
                  />
                ))}
              </div>
            ) : (
              mainList.map((conv) => {
                const children = subagentMap.get(conv.id) || [];
                const isExpanded = expandedParents.has(conv.id);
                return (
                  <div key={conv.id}>
                    <ConversationListItem
                      conv={conv}
                      active={conv.id === selectedId}
                      onClick={() => selectConversation(conv.id)}
                      subagentCount={children.length}
                      expanded={isExpanded}
                      onToggleExpand={() => toggleParentExpand(conv.id)}
                    />
                    {isExpanded && children.map((child) => (
                      <ConversationListItem
                        key={child.id}
                        conv={child}
                        active={child.id === selectedId}
                        onClick={() => selectConversation(child.id)}
                        isSubagent
                      />
                    ))}
                  </div>
                );
              })
            )}
          </div>
        )}
      </div>

      {/* Right: Message detail */}
      <div className={cn(
        'flex-1 flex flex-col min-w-0',
        showList && 'hidden md:flex',
      )}>
        {selectedId && selectedConv ? (
          <>
            {/* Header */}
            <div className="flex items-center gap-3 px-4 py-3 border-b border-neutral-800/50">
              <button
                onClick={() => setShowList(true)}
                className="md:hidden p-1 rounded text-neutral-500 hover:text-neutral-300"
              >
                <ArrowLeft className="w-4 h-4" />
              </button>
              <MessageSquare className="w-4 h-4 text-orange-400" />
              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-2">
                  {selectedConv.parentSessionId && (
                    <button
                      onClick={() => selectConversation(selectedConv.parentSessionId!)}
                      className="flex items-center gap-1 text-[11px] text-neutral-500 hover:text-neutral-300 transition-colors"
                      title="返回父会话"
                    >
                      <GitBranch className="w-3 h-3" />
                      <span>子任务</span>
                    </button>
                  )}
                  {selectedConv.project && (
                    <span className="text-sm font-medium text-neutral-200">{selectedConv.project.split('/').pop()}</span>
                  )}
                  <Badge
                    variant="outline"
                    className={cn(
                      'text-[10px] border-neutral-800',
                      selectedConv.status === 'active' ? 'text-green-500'
                        : selectedConv.status === 'compacted' ? 'text-yellow-600'
                        : 'text-neutral-600',
                    )}
                  >
                    {selectedConv.status === 'active' ? '进行中' : selectedConv.status === 'compacted' ? '已压缩' : '已完成'}
                  </Badge>
                </div>
                <div className="flex items-center gap-3 text-[11px] text-neutral-500">
                  {selectedConv.messageCount > 0 && <span>{selectedConv.messageCount} 条消息</span>}
                  {selectedConv.model && <span className="font-mono">{selectedConv.model}</span>}
                  {selectedConv.slotId && <span className="font-mono text-cyan-500/60">{selectedConv.slotId}</span>}
                  <span>{new Date(selectedConv.startedAt).toLocaleString('zh-CN')}</span>
                </div>
                {selectedConv.llmSummary && (
                  <p className="text-[11px] text-neutral-500 mt-0.5 line-clamp-1">{selectedConv.llmSummary}</p>
                )}
              </div>
              <button
                onClick={() => {
                  const next = !showLabels;
                  setShowLabels(next);
                  if (selectedId) fetchMessages(selectedId, next);
                }}
                className={cn(
                  'flex items-center gap-1 px-2 py-1 rounded-md text-[11px] font-medium transition-colors flex-shrink-0',
                  showLabels
                    ? 'bg-amber-500/10 text-amber-300 border border-amber-500/30'
                    : 'text-neutral-500 hover:text-neutral-300 border border-neutral-800 hover:border-neutral-700',
                )}
                title="显示/隐藏标签"
              >
                <Tag className="w-3 h-3" />
                标签
              </button>
            </div>

            {/* Messages + Events Timeline */}
            <div className="flex-1 overflow-auto px-4 py-2">
              {loadingMessages ? (
                <div className="text-center py-8 text-neutral-600 text-xs">加载消息...</div>
              ) : messages.length === 0 && selectedConv.source === 'router_chat' ? (
                <div className="flex items-center justify-center py-12">
                  <div className="max-w-sm text-center space-y-3">
                    <Sparkles className="w-8 h-8 text-indigo-400/50 mx-auto" />
                    <div className="text-sm text-neutral-400 font-medium">Gemini Router Chat</div>
                    <div className="text-xs text-neutral-600 space-y-1">
                      {selectedConv.taskId && (
                        <div>Task: <span className="font-mono text-indigo-300/70">{selectedConv.taskId}</span></div>
                      )}
                      <div>Model: <span className="font-mono">{selectedConv.model || 'gemini'}</span></div>
                      <div>{new Date(selectedConv.startedAt).toLocaleString('zh-CN')}</div>
                    </div>
                    <p className="text-[11px] text-neutral-600 border border-neutral-800 rounded px-3 py-2">
                      消息已归档或通过滚动摘要压缩。
                    </p>
                    {selectedConv.taskId && (
                      <button
                        onClick={() => {
                          // Navigate to Board tab with this task
                          const boardTab = document.querySelector('[data-tab="board"]') as HTMLElement;
                          if (boardTab) boardTab.click();
                        }}
                        className="text-[11px] text-indigo-400 hover:text-indigo-300 transition-colors"
                      >
                        → 查看关联 Board 任务
                      </button>
                    )}
                  </div>
                </div>
              ) : messages.length === 0 ? (
                <div className="text-center py-8 text-neutral-600 text-xs">暂无消息</div>
              ) : (
                <>
                  {/* Event stats summary */}
                  {events.length > 0 && (
                    <div className="flex items-center gap-3 px-1 py-1.5 mb-2 text-[10px] text-neutral-600 border border-neutral-800/50 rounded">
                      <Layers className="w-3 h-3" />
                      <span>{events.length} 系统事件</span>
                      {(() => {
                        const turns = events.filter((e) => e.eventType === 'turn_duration');
                        if (turns.length === 0) return null;
                        const totalMs = turns.reduce((sum, e) => sum + (parseInt(e.content?.replace('ms', '') || '0') || 0), 0);
                        return <span>{turns.length} turns, 总计 {(totalMs / 1000).toFixed(1)}s</span>;
                      })()}
                      {(() => {
                        const compacts = events.filter((e) => e.eventType === 'compact_boundary').length;
                        return compacts > 0 ? <span>{compacts} 次压缩</span> : null;
                      })()}
                    </div>
                  )}
                  {/* Label stats summary */}
                  {showLabels && Object.keys(labelsMap).length > 0 && (() => {
                    const counts: Record<string, number> = {};
                    for (const pairs of Object.values(labelsMap)) {
                      for (const [label] of pairs) {
                        counts[label] = (counts[label] || 0) + 1;
                      }
                    }
                    return (
                      <div className="flex items-center gap-2 px-1 py-1.5 mb-2 text-[10px] border border-amber-500/20 bg-amber-500/5 rounded flex-wrap">
                        <Tag className="w-3 h-3 text-amber-400 flex-shrink-0" />
                        <span className="text-amber-300">{Object.keys(labelsMap).length} 条消息有标签</span>
                        {Object.entries(counts).sort((a, b) => b[1] - a[1]).map(([label, count]) => {
                          const style = LABEL_STYLES[label] || { text: 'text-neutral-400', bg: 'bg-neutral-500/10', short: label };
                          return (
                            <span key={label} className={cn('px-1.5 py-0.5 rounded font-mono', style.text, style.bg)}>
                              {style.short} {count}
                            </span>
                          );
                        })}
                      </div>
                    );
                  })()}
                  {groupedTimeline.map((group) => (
                    <div key={group.date}>
                      <div className="flex items-center gap-3 my-3">
                        <div className="flex-1 h-px bg-neutral-800/50" />
                        <span className="text-[10px] text-neutral-600">{group.date}</span>
                        <div className="flex-1 h-px bg-neutral-800/50" />
                      </div>
                      {group.items.map((item) =>
                        item.type === 'message' ? (
                          <MessageBubble
                            key={`msg-${item.data.id}`}
                            msg={item.data as ConversationMessage}
                            jsonlPath={jsonlPath}
                            labels={showLabels ? labelsMap[String((item.data as ConversationMessage).id)] : undefined}
                          />
                        ) : (
                          <EventBubble key={`evt-${item.data.id}`} event={item.data as ConversationEvent} />
                        )
                      )}
                    </div>
                  ))}
                  {hasMore && (
                    <div className="flex justify-center py-4">
                      <button
                        onClick={loadMoreMessages}
                        disabled={loadingMore}
                        className="px-4 py-2 text-sm text-neutral-400 bg-neutral-800/50 rounded-lg border border-neutral-700/50 hover:bg-neutral-700/50 hover:text-neutral-300 transition-colors disabled:opacity-50"
                      >
                        {loadingMore ? '加载中...' : `加载更多 (已显示 ${messages.length}${totalMessageCount > 0 ? ` / ${totalMessageCount}` : ''})`}
                      </button>
                    </div>
                  )}
                </>
              )}
            </div>
          </>
        ) : (
          <div className="flex-1 flex items-center justify-center">
            <div className="text-center">
              <MessageSquare className="w-8 h-8 text-neutral-700 mx-auto mb-2" />
              <p className="text-sm text-neutral-600">选择一个对话查看详情</p>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
