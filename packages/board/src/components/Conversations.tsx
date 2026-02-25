'use client';

import { useState, useEffect, useCallback, useMemo } from 'react';
import { Search, RefreshCw, MessageSquare, User, Bot, Wrench, ArrowLeft } from 'lucide-react';
import { cn } from '@/lib/utils';
import { Badge } from '@/components/ui/badge';

interface Conversation {
  id: string;
  project: string | null;
  slotId: string | null;
  source: string;
  model: string | null;
  gitBranch: string | null;
  messageCount: number;
  startedAt: string;
  endedAt: string | null;
  status: string;
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
  assistant: { icon: Bot, color: 'text-green-400', label: 'AI' },
  tool_use: { icon: Wrench, color: 'text-amber-400', label: '工具调用' },
  tool_result: { icon: Wrench, color: 'text-neutral-500', label: '工具结果' },
};

function MessageBubble({ msg }: { msg: ConversationMessage }) {
  const [expanded, setExpanded] = useState(false);
  const config = ROLE_CONFIG[msg.role] || ROLE_CONFIG.assistant;
  const Icon = config.icon;
  const isToolResult = msg.role === 'tool_result';
  const isToolUse = msg.role === 'tool_use';
  const contentPreview = msg.content.length > 500 && !expanded
    ? msg.content.slice(0, 500) + '...'
    : msg.content;

  return (
    <div className={cn(
      'group flex gap-2.5 py-2',
      isToolResult && 'opacity-60',
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
          )}
          onClick={() => msg.content.length > 500 && setExpanded(!expanded)}
        >
          {contentPreview}
        </div>
        {msg.content.length > 500 && (
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

function ConversationListItem({
  conv,
  active,
  onClick,
}: {
  conv: Conversation;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      className={cn(
        'w-full text-left p-3 rounded-lg border transition-colors',
        active
          ? 'bg-neutral-800/50 border-orange-500/30'
          : 'border-neutral-800/50 hover:border-neutral-700',
      )}
    >
      <div className="flex items-center justify-between mb-1">
        <div className="flex items-center gap-2 min-w-0">
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
        <Badge
          variant="outline"
          className={cn(
            'text-[10px] border-neutral-800 flex-shrink-0',
            conv.status === 'active' ? 'text-green-500' : 'text-neutral-600',
          )}
        >
          {conv.status === 'active' ? '进行中' : '已完成'}
        </Badge>
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
  );
}

export function Conversations() {
  const [conversations, setConversations] = useState<Conversation[]>([]);
  const [messages, setMessages] = useState<ConversationMessage[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadingMessages, setLoadingMessages] = useState(false);
  const [search, setSearch] = useState('');
  const [searchResults, setSearchResults] = useState<ConversationMessage[] | null>(null);
  const [statusFilter, setStatusFilter] = useState<string | null>(null);
  const [showList, setShowList] = useState(true); // mobile: toggle list/detail

  const fetchConversations = useCallback(async () => {
    setLoading(true);
    try {
      const params = new URLSearchParams();
      if (statusFilter) params.set('status', statusFilter);
      params.set('limit', '100');
      const res = await fetch(`/api/conversations?${params}`);
      if (res.ok) {
        const data = await res.json();
        setConversations(Array.isArray(data) ? data : []);
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
      }
    } catch {
      setMessages([]);
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

  // Group messages by date
  const groupedMessages = useMemo(() => {
    const groups: { date: string; messages: ConversationMessage[] }[] = [];
    let currentDate = '';
    for (const msg of messages) {
      const date = formatDate(msg.timestamp);
      if (date !== currentDate) {
        currentDate = date;
        groups.push({ date, messages: [msg] });
      } else {
        groups[groups.length - 1].messages.push(msg);
      }
    }
    return groups;
  }, [messages]);

  const counts = useMemo(() => {
    const active = conversations.filter((c) => c.status === 'active').length;
    const completed = conversations.filter((c) => c.status === 'completed').length;
    return { active, completed, total: conversations.length };
  }, [conversations]);

  return (
    <div className="flex-1 flex min-h-0 overflow-hidden">
      {/* Left: Conversation list */}
      <div className={cn(
        'w-80 flex-shrink-0 border-r border-neutral-800 flex flex-col',
        !showList && 'hidden md:flex',
      )}>
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
            ) : conversations.length === 0 ? (
              <div className="text-center py-8 text-neutral-600 text-xs">暂无对话记录</div>
            ) : (
              conversations.map((conv) => (
                <ConversationListItem
                  key={conv.id}
                  conv={conv}
                  active={conv.id === selectedId}
                  onClick={() => selectConversation(conv.id)}
                />
              ))
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
                  {selectedConv.project && (
                    <span className="text-sm font-medium text-neutral-200">{selectedConv.project.split('/').pop()}</span>
                  )}
                  <Badge
                    variant="outline"
                    className={cn(
                      'text-[10px] border-neutral-800',
                      selectedConv.status === 'active' ? 'text-green-500' : 'text-neutral-600',
                    )}
                  >
                    {selectedConv.status === 'active' ? '进行中' : '已完成'}
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

            {/* Messages */}
            <div className="flex-1 overflow-auto px-4 py-2">
              {loadingMessages ? (
                <div className="text-center py-8 text-neutral-600 text-xs">加载消息...</div>
              ) : messages.length === 0 ? (
                <div className="text-center py-8 text-neutral-600 text-xs">暂无消息</div>
              ) : (
                groupedMessages.map((group) => (
                  <div key={group.date}>
                    <div className="flex items-center gap-3 my-3">
                      <div className="flex-1 h-px bg-neutral-800/50" />
                      <span className="text-[10px] text-neutral-600">{group.date}</span>
                      <div className="flex-1 h-px bg-neutral-800/50" />
                    </div>
                    {group.messages.map((msg) => (
                      <MessageBubble key={msg.id} msg={msg} />
                    ))}
                  </div>
                ))
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
