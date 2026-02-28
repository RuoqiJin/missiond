'use client';

import { useState, useEffect, useCallback, useMemo } from 'react';
import { Search, RefreshCw, MessageSquare, User, Bot, Wrench, ArrowLeft, ChevronRight, ChevronDown, GitBranch, Terminal, Brain, Timer, Layers, Zap } from 'lucide-react';
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
}

interface ConversationMessage {
  id: number;
  sessionId: string;
  role: string;
  content: string;
  rawContent: string | null;
  messageUuid: string | null;
  model: string | null;
  timestamp: string;
  metadata: string | null;
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

function MessageBubble({ msg, jsonlPath }: { msg: ConversationMessage; jsonlPath?: string | null }) {
  const [expanded, setExpanded] = useState(false);
  const config = ROLE_CONFIG[msg.role] || ROLE_CONFIG.assistant;
  const Icon = config.icon;
  const isToolResult = msg.role === 'tool_result';
  const isToolUse = msg.role === 'tool_use';
  const isThinking = msg.role === 'thinking';

  // Check if this message has images (use rich rendering for those)
  const hasImages = msg.rawContent?.includes('"type":"image"') || msg.content.includes('[图片]');
  // Thinking messages: default collapsed at 300 chars; others at 500
  const collapseThreshold = isThinking ? 300 : 500;
  const contentPreview = !hasImages && msg.content.length > collapseThreshold && !expanded
    ? msg.content.slice(0, collapseThreshold) + '...'
    : msg.content;

  return (
    <div className={cn(
      'group flex gap-2.5 py-2',
      (isToolResult || isThinking) && 'opacity-60',
    )}>
      <div className={cn('flex-shrink-0 mt-1 p-1 rounded', config.color)}>
        <Icon className="w-3.5 h-3.5" />
      </div>
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2 mb-0.5">
          <span className={cn('text-[11px] font-medium', config.color)}>{config.label}</span>
          <span className="text-[10px] text-neutral-600">{formatTime(msg.timestamp)}</span>
          {msg.model && (
            <span className="text-[10px] text-neutral-700 font-mono">{msg.model}</span>
          )}
        </div>
        <div
          className={cn(
            'text-sm leading-relaxed whitespace-pre-wrap break-words',
            msg.role === 'user' ? 'text-neutral-200' : 'text-neutral-400',
            (isToolUse || isToolResult) && 'font-mono text-xs',
            isThinking && 'text-xs italic text-purple-300/60',
          )}
          onClick={() => !hasImages && msg.content.length > collapseThreshold && setExpanded(!expanded)}
        >
          {hasImages ? (
            <MessageContent msg={msg} jsonlPath={jsonlPath} />
          ) : (
            contentPreview
          )}
        </div>
        {!hasImages && msg.content.length > collapseThreshold && (
          <button
            onClick={() => setExpanded(!expanded)}
            className="text-[11px] text-neutral-600 hover:text-neutral-400 mt-1"
          >
            {expanded ? '收起' : '展开全部'}
          </button>
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

        <div className="flex items-center justify-between text-[11px] text-neutral-500">
          <div className="flex items-center gap-2">
            <span>{conv.messageCount} 条消息</span>
            {conv.source === 'pty' && <span className="text-purple-500/60">PTY</span>}
            {conv.model && (
              <span className="font-mono text-neutral-600 truncate max-w-[100px]">{conv.model}</span>
            )}
          </div>
          <span>{timeAgo(conv.startedAt)}</span>
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

export function Conversations() {
  const [conversations, setConversations] = useState<Conversation[]>([]);
  const [messages, setMessages] = useState<ConversationMessage[]>([]);
  const [events, setEvents] = useState<ConversationEvent[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [jsonlPath, setJsonlPath] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadingMessages, setLoadingMessages] = useState(false);
  const [search, setSearch] = useState('');
  const [searchResults, setSearchResults] = useState<ConversationMessage[] | null>(null);
  const [statusFilter, setStatusFilter] = useState<string | null>(null);
  const [viewMode, setViewMode] = useState<'user' | 'system'>('user');
  const [showList, setShowList] = useState(true); // mobile: toggle list/detail
  const [expandedParents, setExpandedParents] = useState<Set<string>>(new Set());

  const fetchConversations = useCallback(async () => {
    setLoading(true);
    try {
      const params = new URLSearchParams();
      if (statusFilter) params.set('status', statusFilter);
      params.set('limit', '100');
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

  const fetchMessages = useCallback(async (sessionId: string) => {
    setLoadingMessages(true);
    setSearchResults(null);
    try {
      const res = await fetch(`/api/conversations?sessionId=${encodeURIComponent(sessionId)}&tail=500`);
      if (res.ok) {
        const data = await res.json();
        setMessages(data.messages || []);
        setEvents(data.events || []);
        setJsonlPath(data.conversation?.jsonlPath || null);
      }
    } catch {
      setMessages([]);
      setEvents([]);
      setJsonlPath(null);
    }
    setLoadingMessages(false);
  }, []);

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
    fetchMessages(id);
  }, [fetchMessages]);

  const selectedConv = useMemo(
    () => conversations.find((c) => c.id === selectedId),
    [conversations, selectedId],
  );

  // Merge messages and important events into a unified timeline grouped by date
  type TimelineItem = { type: 'message'; data: ConversationMessage } | { type: 'event'; data: ConversationEvent };
  const groupedTimeline = useMemo(() => {
    // Filter events: only show important ones inline (turn_duration, compact_boundary, queue ops)
    // Skip high-frequency noise (bash_progress, mcp_progress, hook_progress, waiting_for_task)
    const importantEvents = events.filter((e) => {
      const t = e.eventType;
      return t === 'turn_duration' || t === 'compact_boundary' || t.startsWith('queue:');
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

  const counts = useMemo(() => {
    const filtered = conversations.filter((c) =>
      viewMode === 'user' ? c.conversationType === 'user' : c.conversationType !== 'user'
    );
    const active = filtered.filter((c) => c.status === 'active').length;
    const completed = filtered.filter((c) => c.status === 'completed').length;
    const compacted = filtered.filter((c) => c.status === 'compacted').length;
    return { active, completed, compacted, total: filtered.length };
  }, [conversations, viewMode]);

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
        // Hide compacted sessions from main list (context compaction fragments)
        continue;
      } else if (viewMode === 'user' && conv.conversationType !== 'user') {
        continue;
      } else if (viewMode === 'system' && conv.conversationType === 'user') {
        continue;
      } else {
        main.push(conv);
      }
    }
    return { mainList: main, subagentMap: map };
  }, [conversations, viewMode]);

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
            onClick={() => setViewMode('user')}
            className={cn(
              'flex-1 py-2 text-xs font-medium transition-colors flex items-center justify-center gap-1.5',
              viewMode === 'user'
                ? 'text-neutral-200 border-b-2 border-blue-500'
                : 'text-neutral-500 hover:text-neutral-400',
            )}
          >
            <MessageSquare className="w-3 h-3" />
            对话
          </button>
          <button
            onClick={() => setViewMode('system')}
            className={cn(
              'flex-1 py-2 text-xs font-medium transition-colors flex items-center justify-center gap-1.5',
              viewMode === 'system'
                ? 'text-neutral-200 border-b-2 border-orange-500'
                : 'text-neutral-500 hover:text-neutral-400',
            )}
          >
            <Layers className="w-3 h-3" />
            系统
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
              <div className="text-center py-8 text-neutral-600 text-xs">暂无对话记录</div>
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
                  <span>{selectedConv.messageCount} 条消息</span>
                  {selectedConv.model && <span className="font-mono">{selectedConv.model}</span>}
                  {selectedConv.slotId && <span>{selectedConv.slotId}</span>}
                  <span>{new Date(selectedConv.startedAt).toLocaleString('zh-CN')}</span>
                </div>
              </div>
            </div>

            {/* Messages + Events Timeline */}
            <div className="flex-1 overflow-auto px-4 py-2">
              {loadingMessages ? (
                <div className="text-center py-8 text-neutral-600 text-xs">加载消息...</div>
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
                  {groupedTimeline.map((group) => (
                    <div key={group.date}>
                      <div className="flex items-center gap-3 my-3">
                        <div className="flex-1 h-px bg-neutral-800/50" />
                        <span className="text-[10px] text-neutral-600">{group.date}</span>
                        <div className="flex-1 h-px bg-neutral-800/50" />
                      </div>
                      {group.items.map((item) =>
                        item.type === 'message' ? (
                          <MessageBubble key={`msg-${item.data.id}`} msg={item.data as ConversationMessage} jsonlPath={jsonlPath} />
                        ) : (
                          <EventBubble key={`evt-${item.data.id}`} event={item.data as ConversationEvent} />
                        )
                      )}
                    </div>
                  ))}
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
