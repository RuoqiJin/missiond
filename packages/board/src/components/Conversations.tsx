"use client";

import { useState, useEffect, useCallback, useMemo, useRef } from "react";
import { Virtuoso, type VirtuosoHandle, type ListRange } from "react-virtuoso";
import {
  Search,
  RefreshCw,
  MessageSquare,
  User,
  Bot,
  Wrench,
  ArrowLeft,
  ChevronRight,
  ChevronDown,
  ChevronsDownUp,
  ChevronsUpDown,
  GitBranch,
  Terminal,
  Brain,
  Timer,
  Layers,
  Zap,
  Tag,
  Sparkles,
  Server,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import { MarkdownContent } from "@/components/timeline/MarkdownContent";

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

interface ConversationTurn {
  turnIdx: number;
  startMessageId: number;
  endMessageId: number;
  userContent: string | null;
  toolNames: string | null;
  toolCallCount: number;
  messageCount: number;
  hasCodeChange: boolean;
  hasMcpCall: boolean;
  startedAt: string | null;
  endedAt: string | null;
  topic: string | null;
}

function timeAgo(dateStr: string): string {
  const diff = Date.now() - new Date(dateStr).getTime();
  const mins = Math.floor(diff / 60000);
  if (mins < 1) return "刚刚";
  if (mins < 60) return `${mins}分前`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}时前`;
  const days = Math.floor(hours / 24);
  if (days < 30) return `${days}天前`;
  return new Date(dateStr).toLocaleDateString("zh-CN");
}

function formatTime(dateStr: string): string {
  const d = new Date(dateStr);
  return d.toLocaleTimeString("zh-CN", {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

function formatDate(dateStr: string): string {
  const d = new Date(dateStr);
  return d.toLocaleDateString("zh-CN", { month: "short", day: "numeric" });
}

function getDayKey(dateStr: string): string {
  const d = new Date(dateStr);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

function getDayLabel(dayKey: string): string {
  const today = new Date();
  const todayKey = getDayKey(today.toISOString());
  if (dayKey === todayKey) return "今天";
  const yesterday = new Date(today);
  yesterday.setDate(yesterday.getDate() - 1);
  if (dayKey === getDayKey(yesterday.toISOString())) return "昨天";
  const d = new Date(dayKey + "T00:00:00");
  const diffDays = Math.floor((today.getTime() - d.getTime()) / 86400000);
  if (diffDays < 7) {
    const weekdays = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"];
    return weekdays[d.getDay()];
  }
  return d.toLocaleDateString("zh-CN", { month: "short", day: "numeric" });
}

function groupByDay(
  list: Conversation[],
): { dayKey: string; label: string; items: Conversation[] }[] {
  const groups: Map<string, Conversation[]> = new Map();
  for (const conv of list) {
    const key = getDayKey(conv.startedAt);
    const arr = groups.get(key) || [];
    arr.push(conv);
    groups.set(key, arr);
  }
  return Array.from(groups.entries())
    .sort(([a], [b]) => b.localeCompare(a))
    .map(([dayKey, items]) => ({
      dayKey,
      label: getDayLabel(dayKey),
      items,
    }));
}

const ROLE_CONFIG: Record<
  string,
  { icon: typeof User; color: string; label: string }
> = {
  user: { icon: User, color: "text-blue-400", label: "用户" },
  system: { icon: Terminal, color: "text-orange-400", label: "系统指令" },
  assistant: { icon: Bot, color: "text-green-400", label: "AI" },
  tool_use: { icon: Wrench, color: "text-amber-400", label: "工具调用" },
  tool_result: { icon: Wrench, color: "text-neutral-500", label: "工具结果" },
  thinking: { icon: Brain, color: "text-purple-400", label: "思考" },
  agent_user: { icon: User, color: "text-cyan-400", label: "Agent 用户" },
  agent_assistant: { icon: Bot, color: "text-teal-400", label: "Agent AI" },
};

function ImageBlock({
  jsonlPath,
  messageUuid,
  imageIndex,
}: {
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
          "rounded-lg border border-neutral-700 cursor-pointer transition-all hover:border-neutral-500",
          expanded ? "max-w-full" : "max-w-sm max-h-64 object-cover",
        )}
        onClick={() => setExpanded(!expanded)}
        loading="lazy"
      />
    </div>
  );
}

/** Render message content with inline images from rawContent when available */
function MessageContent({
  msg,
  jsonlPath,
}: {
  msg: ConversationMessage;
  jsonlPath?: string | null;
}) {
  const blocks = useMemo(() => {
    if (!msg.rawContent) return null;
    try {
      const raw = JSON.parse(msg.rawContent);
      if (!Array.isArray(raw)) return null;
      // Only use rich rendering if there are image blocks
      if (!raw.some((b: Record<string, unknown>) => b.type === "image"))
        return null;
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
          if (block.type === "text") {
            return <span key={i}>{block.text as string}</span>;
          }
          if (block.type === "image") {
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
          if (block.type === "tool_use") {
            return (
              <span key={i} className="text-amber-400/70">
                [Tool: {block.name as string}]
              </span>
            );
          }
          return null;
        })}
      </>
    );
  }

  // Fallback: plain text
  return <>{msg.content}</>;
}

const LABEL_STYLES: Record<
  string,
  { text: string; bg: string; short: string }
> = {
  has_tool_use: {
    text: "text-amber-300",
    bg: "bg-amber-500/10",
    short: "tool_use",
  },
  has_tool_result: {
    text: "text-neutral-400",
    bg: "bg-neutral-500/10",
    short: "tool_result",
  },
  has_code_change: {
    text: "text-green-300",
    bg: "bg-green-500/10",
    short: "code_change",
  },
  has_mcp_call: { text: "text-cyan-300", bg: "bg-cyan-500/10", short: "mcp" },
  has_image: { text: "text-pink-300", bg: "bg-pink-500/10", short: "image" },
  role_mapped: {
    text: "text-purple-300",
    bg: "bg-purple-500/10",
    short: "role",
  },
  gemini_chat: { text: "text-blue-300", bg: "bg-blue-500/10", short: "gemini" },
  gemini_channel: {
    text: "text-indigo-300",
    bg: "bg-indigo-500/10",
    short: "channel",
  },
};

function LabelBadges({ labels }: { labels: [string, string][] }) {
  if (!labels || labels.length === 0) return null;
  return (
    <div className="flex items-center gap-1 flex-wrap">
      {labels.map(([label, value]) => {
        const style = LABEL_STYLES[label] || {
          text: "text-neutral-400",
          bg: "bg-neutral-500/10",
          short: label,
        };
        const display =
          value === "true" ? style.short : `${style.short}:${value}`;
        return (
          <span
            key={label}
            className={cn(
              "px-1.5 py-0.5 text-[9px] font-mono rounded",
              style.text,
              style.bg,
            )}
          >
            {display}
          </span>
        );
      })}
    </div>
  );
}

// ─── Tool-pair viewer components ───

/** Strip `N→` line number prefixes from cat -n output */
function stripLineNumbers(text: string): string {
  return text.replace(/^ *\d+→/gm, "");
}

/** Infer language from file extension */
function inferLang(filePath: string): string {
  const ext = filePath.split(".").pop()?.toLowerCase() || "";
  const map: Record<string, string> = {
    rs: "rust",
    ts: "typescript",
    tsx: "tsx",
    js: "javascript",
    jsx: "jsx",
    py: "python",
    rb: "ruby",
    go: "go",
    java: "java",
    sh: "bash",
    zsh: "bash",
    yml: "yaml",
    yaml: "yaml",
    toml: "toml",
    json: "json",
    sql: "sql",
    md: "markdown",
    css: "css",
    html: "html",
    swift: "swift",
  };
  return map[ext] || ext;
}

/** Parse tool_use content to extract parameters */
function parseToolCall(content: string): Record<string, string> {
  // Format: [Tool: Name] key: "value", key2: value
  const params: Record<string, string> = {};
  const body = content.replace(/^\[Tool: \w+\]\s*/, "");
  // Extract quoted values: key: "value"
  for (const m of body.matchAll(/(\w+):\s*"([^"]*)"/g)) {
    params[m[1]] = m[2];
  }
  // Extract unquoted values: key: value (before next comma)
  for (const m of body.matchAll(/(\w+):\s*([^",\s]+)/g)) {
    if (!params[m[1]]) params[m[1]] = m[2];
  }
  return params;
}

/** File viewer for Read tool results */
function FileViewer({
  filePath,
  content,
}: {
  filePath: string;
  content: string;
}) {
  const [expanded, setExpanded] = useState(false);
  const lang = inferLang(filePath);
  const cleaned = stripLineNumbers(content);
  const lineCount = cleaned.split("\n").length;
  const fileName = filePath.split("/").pop() || filePath;
  const isMarkdown = lang === "markdown";

  return (
    <div className="border border-neutral-800/60 rounded-md overflow-hidden my-1">
      <button
        onClick={() => setExpanded((v) => !v)}
        className="w-full flex items-center gap-2 px-3 py-1.5 text-left bg-neutral-900/80 hover:bg-neutral-800/60 transition-colors"
      >
        <ChevronRight
          className={cn(
            "w-3 h-3 text-neutral-500 transition-transform",
            expanded && "rotate-90",
          )}
        />
        <span className="text-xs font-mono text-cyan-400 truncate">
          {fileName}
        </span>
        <span className="text-[10px] text-neutral-600 truncate hidden sm:inline">
          {filePath}
        </span>
        <span className="text-[10px] text-neutral-600 ml-auto flex-shrink-0">
          {lineCount} lines · {lang}
        </span>
      </button>
      {expanded &&
        (isMarkdown ? (
          <div className="px-3 py-2 max-h-[600px] overflow-auto">
            <MarkdownContent content={cleaned} />
          </div>
        ) : (
          <pre className="px-3 py-2 text-xs font-mono overflow-auto max-h-[600px] bg-neutral-950/50 text-neutral-300">
            {cleaned.split("\n").map((line, i) => (
              <div key={i} className="flex">
                <span className="w-10 text-right pr-3 text-neutral-700 select-none flex-shrink-0">
                  {i + 1}
                </span>
                <span className="flex-1">{line}</span>
              </div>
            ))}
          </pre>
        ))}
    </div>
  );
}

/** Diff viewer for Edit tool results */
function DiffViewer({
  filePath,
  result,
}: {
  filePath: string;
  result: string;
}) {
  const [expanded, setExpanded] = useState(true);
  const fileName = filePath.split("/").pop() || filePath;
  // result is usually "The file ... has been updated successfully." — not much to show
  // But we can at least display the confirmation with file context
  const isSuccess = result.includes("updated successfully");

  return (
    <div className="border border-neutral-800/60 rounded-md overflow-hidden my-1">
      <button
        onClick={() => setExpanded((v) => !v)}
        className="w-full flex items-center gap-2 px-3 py-1.5 text-left bg-neutral-900/80 hover:bg-neutral-800/60 transition-colors"
      >
        <ChevronRight
          className={cn(
            "w-3 h-3 text-neutral-500 transition-transform",
            expanded && "rotate-90",
          )}
        />
        <span className="text-xs font-mono text-amber-400">{fileName}</span>
        {isSuccess && (
          <span className="text-[10px] text-green-500">✓ updated</span>
        )}
        <span className="text-[10px] text-neutral-600 truncate hidden sm:inline ml-auto">
          {filePath}
        </span>
      </button>
      {expanded && (
        <div className="px-3 py-2 text-xs text-neutral-400 whitespace-pre-wrap">
          {result}
        </div>
      )}
    </div>
  );
}

/** Terminal viewer for Bash tool results */
function TerminalViewer({
  command,
  description,
  result,
}: {
  command: string;
  description?: string;
  result: string;
}) {
  const lines = result.split("\n");
  const isShort = lines.length <= 15;
  const [expanded, setExpanded] = useState(isShort);

  return (
    <div className="border border-neutral-800/60 rounded-md overflow-hidden my-1">
      <button
        onClick={() => setExpanded((v) => !v)}
        className="w-full flex items-center gap-2 px-3 py-1.5 text-left bg-neutral-900/80 hover:bg-neutral-800/60 transition-colors"
      >
        <ChevronRight
          className={cn(
            "w-3 h-3 text-neutral-500 transition-transform",
            expanded && "rotate-90",
          )}
        />
        <Terminal className="w-3 h-3 text-green-500/70" />
        <span className="text-xs font-mono text-green-400 truncate">
          {description || command.slice(0, 60)}
        </span>
        <span className="text-[10px] text-neutral-600 ml-auto flex-shrink-0">
          {lines.length} lines
        </span>
      </button>
      {expanded && (
        <div className="bg-neutral-950/80">
          <div className="px-3 py-1 text-[10px] font-mono text-neutral-600 border-b border-neutral-800/50 break-all">
            $ {command}
          </div>
          <pre className="px-3 py-2 text-xs font-mono overflow-auto max-h-[400px] text-neutral-300 whitespace-pre-wrap">
            {result}
          </pre>
        </div>
      )}
    </div>
  );
}

/** Fallback renderer for tool pairs with large results — collapsed by default */
function ToolPairFallback({
  call,
  result,
}: {
  call: ConversationMessage;
  result: ConversationMessage;
}) {
  const THRESHOLD = 500;
  const isLarge = result.content.length > THRESHOLD;
  const [expanded, setExpanded] = useState(!isLarge);
  return (
    <div className="space-y-1">
      <div className="text-xs text-neutral-500 font-mono whitespace-pre-wrap break-words">
        {call.content}
      </div>
      {isLarge && !expanded ? (
        <button
          onClick={() => setExpanded(true)}
          className="text-[11px] text-cyan-500 hover:text-cyan-400 border-l-2 border-neutral-800 pl-2"
        >
          ▶ 展开结果 ({result.content.length.toLocaleString()} chars)
        </button>
      ) : (
        <div className="text-xs text-neutral-400 whitespace-pre-wrap break-words border-l-2 border-neutral-800 pl-2 max-h-[300px] overflow-auto">
          {result.content}
        </div>
      )}
    </div>
  );
}

/** Combined tool call + result renderer */
function ToolPairBubble({
  call,
  result,
  labels,
}: {
  call: ConversationMessage;
  result: ConversationMessage;
  labels?: [string, string][];
}) {
  const toolName = call.toolName || "";
  const params = parseToolCall(call.content);
  const config = ROLE_CONFIG[call.role] || ROLE_CONFIG.assistant;
  const Icon = config.icon;
  const [expanded, setExpanded] = useState(false);

  // Build a short preview for collapsed state
  const preview = toolName === "Read" && params.file_path ? params.file_path
    : toolName === "Edit" && params.file_path ? params.file_path
    : toolName === "Bash" && params.command ? params.command.slice(0, 80)
    : call.content.replace(/^\[Tool: \w+\]\s*/, "").slice(0, 80);

  return (
    <div className="border-b border-neutral-800/30 py-2">
      {/* Compact header — clickable to toggle */}
      <div
        className="flex items-center gap-2 flex-wrap cursor-pointer select-none"
        onClick={() => setExpanded((v) => !v)}
      >
        <ChevronRight
          className={cn(
            "w-3 h-3 text-neutral-600 transition-transform",
            expanded && "rotate-90",
          )}
        />
        <Icon className={cn("w-3.5 h-3.5", config.color)} />
        <span className={cn("text-sm font-semibold", config.color)}>
          🔧 工具调用 (msg {call.id})
        </span>
        <span className="text-xs font-mono px-1.5 py-0.5 rounded bg-neutral-800 text-amber-300">
          {toolName}
        </span>
        {labels && labels.length > 0 && <LabelBadges labels={labels} />}
        {!expanded && (
          <span className="text-xs text-neutral-600 truncate max-w-[400px] ml-1 font-mono">
            {preview}
          </span>
        )}
        <span className="text-[10px] text-cyan-500/50 font-mono ml-auto">
          {call.timestamp.split("T")[1]?.split(".")[0] || call.timestamp}
        </span>
      </div>

      {/* Specialized viewer based on tool type — only when expanded */}
      {expanded && (
        <>
          {toolName === "Read" && params.file_path ? (
            <FileViewer filePath={params.file_path} content={result.content} />
          ) : toolName === "Edit" && params.file_path ? (
            <DiffViewer filePath={params.file_path} result={result.content} />
          ) : toolName === "Bash" && params.command ? (
            <TerminalViewer
              command={params.command}
              description={params.description}
              result={result.content}
            />
          ) : (
            <ToolPairFallback call={call} result={result} />
          )}
        </>
      )}
    </div>
  );
}

/** Minimap sidebar: full user message index with scroll-tracking highlight and on-demand loading */
function UserMessageMinimap({
  userIndex,
  flatTimeline,
  visibleStartIndex,
  onJump,
  onLoadAround,
}: {
  userIndex: { id: number; time: string; preview: string }[];
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  flatTimeline: any[];
  visibleStartIndex: number;
  onJump: (index: number) => void;
  onLoadAround: (messageId: number) => void;
}) {
  const activeRef = useRef<HTMLButtonElement | null>(null);
  const allMarkers = useMemo(
    () => userIndex.map((it) => ({ ...it, preview: (it.preview || "").replace(/\n/g, " ") })),
    [userIndex],
  );

  // Build a Set of loaded message IDs for quick lookup
  const loadedIds = useMemo(() => {
    const s = new Set<number>();
    for (const item of flatTimeline) {
      if (item.type === "message") s.add(item.data.id);
      else if (item.type === "tool-pair") { s.add(item.call.id); s.add(item.result.id); }
    }
    return s;
  }, [flatTimeline]);

  // Find which user message is currently visible (closest to visibleStartIndex)
  const activeMarkerId = useMemo(() => {
    // Walk backward from visibleStartIndex to find the nearest user message
    for (let i = Math.min(visibleStartIndex + 5, flatTimeline.length - 1); i >= 0; i--) {
      const item = flatTimeline[i];
      if (item?.type === "message" && item.data?.role === "user") {
        return item.data.id as number;
      }
    }
    return null;
  }, [flatTimeline, visibleStartIndex]);

  // Auto-scroll the minimap to keep the active marker visible
  useEffect(() => {
    activeRef.current?.scrollIntoView({ block: "nearest" });
  }, [activeMarkerId]);

  if (allMarkers.length === 0) return null;

  return (
    <div className="w-36 flex-shrink-0 border-l border-neutral-800/50 overflow-y-auto">
      <div className="sticky top-0 bg-neutral-950 px-2 py-1.5 text-[9px] text-neutral-600 border-b border-neutral-800/30 z-10">
        👤 {allMarkers.length} 条用户消息
      </div>
      <div className="py-1">
        {allMarkers.map((m) => {
          const isLoaded = loadedIds.has(m.id);
          const isActive = m.id === activeMarkerId;
          return (
            <button
              key={m.id}
              ref={isActive ? activeRef : undefined}
              onClick={() => {
                if (isLoaded) {
                  // Find index in flatTimeline and scroll
                  const idx = flatTimeline.findIndex(
                    (it: { type: string; data?: { id: number } }) =>
                      it.type === "message" && it.data?.id === m.id,
                  );
                  if (idx >= 0) onJump(idx);
                } else {
                  onLoadAround(m.id);
                }
              }}
              className={cn(
                "w-full text-left px-2 py-1 transition-colors",
                isActive
                  ? "bg-blue-500/20 border-l-2 border-blue-400"
                  : "hover:bg-neutral-800/50 border-l-2 border-transparent",
                !isLoaded && "opacity-50",
              )}
              title={isLoaded ? m.preview : `${m.preview}\n(点击加载此区域)`}
            >
              <div className="flex items-center gap-1">
                <div className={cn(
                  "w-1.5 h-1.5 rounded-full flex-shrink-0",
                  isActive ? "bg-blue-400" : "bg-blue-400/50",
                )} />
                <span className={cn("text-[9px]", isActive ? "text-blue-300" : "text-neutral-600")}>{m.time}</span>
              </div>
              <p className={cn(
                "text-[10px] truncate pl-2.5",
                isActive ? "text-blue-200/80" : "text-neutral-600",
              )}>
                {m.preview || "(空)"}
              </p>
            </button>
          );
        })}
      </div>
    </div>
  );
}

// Roles that are collapsed by default — only show a summary header
const COLLAPSED_ROLES = new Set([
  "thinking",
  "agent_user",
  "agent_assistant",
  "tool_result",
]);

function MessageBubble({
  msg,
  jsonlPath,
  labels,
}: {
  msg: ConversationMessage;
  jsonlPath?: string | null;
  labels?: [string, string][];
}) {
  const isCollapsible = COLLAPSED_ROLES.has(msg.role);
  const [expanded, setExpanded] = useState(!isCollapsible);
  const [showFull, setShowFull] = useState(false);

  const isSlot = msg.roleDisplay?.startsWith("slot-");
  const config = isSlot
    ? {
        icon: Terminal,
        color: "text-orange-400",
        label: msg.roleDisplay as string,
      }
    : ROLE_CONFIG[msg.role] || ROLE_CONFIG.assistant;
  const Icon = config.icon;

  // Check if this message has images (use rich rendering for those)
  const hasImages =
    msg.rawContent?.includes('"type":"image"') ||
    msg.content.includes("[图片]");

  // Role display with emoji
  const roleEmoji: Record<string, string> = {
    user: "👤",
    assistant: "🤖",
    tool_use: "🔧",
    tool_result: "🔧",
    thinking: "🧠",
    system: "⚙️",
    agent_user: "👤",
    agent_assistant: "🤖",
    compact_summary: "📋",
  };

  // Extract a short preview for collapsed state
  const preview = isCollapsible
    ? msg.content.slice(0, 80).replace(/\n/g, " ") +
      (msg.content.length > 80 ? "…" : "")
    : "";

  return (
    <div className="border-b border-neutral-800/30 py-3">
      {/* Header: role + msg ID + timestamp + tool */}
      <div
        className={cn(
          "flex items-center gap-2 flex-wrap",
          isCollapsible && "cursor-pointer select-none",
        )}
        onClick={isCollapsible ? () => setExpanded((v) => !v) : undefined}
      >
        {isCollapsible && (
          <ChevronRight
            className={cn(
              "w-3 h-3 text-neutral-600 transition-transform",
              expanded && "rotate-90",
            )}
          />
        )}
        <Icon className={cn("w-3.5 h-3.5", config.color)} />
        <span className={cn("text-sm font-semibold", config.color)}>
          {isSlot ? "⚙️" : roleEmoji[msg.role] || "📄"}{" "}
          {isSlot ? msg.roleDisplay : msg.roleDisplay || config.label} (msg{" "}
          {msg.id})
        </span>
        {msg.toolName && (
          <span className="text-xs font-mono px-1.5 py-0.5 rounded bg-neutral-800 text-amber-300">
            {msg.toolName}
          </span>
        )}
        {labels && labels.length > 0 && <LabelBadges labels={labels} />}
        {!expanded && (
          <span className="text-xs text-neutral-600 truncate max-w-[400px] ml-1">
            {preview}
          </span>
        )}
        {!expanded && (
          <span className="text-[10px] text-neutral-700 ml-auto flex-shrink-0">
            {msg.content.length.toLocaleString()} chars
          </span>
        )}
      </div>

      {expanded && (
        <>
          {/* Timestamp line */}
          <div className="text-xs text-cyan-500/70 mb-2 mt-1.5 font-mono">
            {msg.timestamp}
            {msg.model && (
              <span className="ml-3 text-neutral-600">{msg.model}</span>
            )}
            {msg.seq != null && (
              <span className="ml-3 text-neutral-700">seq:{msg.seq}</span>
            )}
          </div>

          {/* Content */}
          <div
            className={cn(
              "text-sm leading-relaxed whitespace-pre-wrap break-words",
              msg.role === "user" ? "text-neutral-200" : "text-neutral-400",
              msg.role === "thinking" && "text-purple-300/70",
              msg.role === "tool_result" && "font-mono text-xs",
            )}
          >
            {hasImages ? (
              <MessageContent msg={msg} jsonlPath={jsonlPath} />
            ) : msg.content.length > 2000 && !showFull ? (
              <>
                <div>{msg.content.slice(0, 2000)}</div>
                <button
                  onClick={() => setShowFull(true)}
                  className="text-[11px] text-cyan-500 hover:text-cyan-400 mt-1"
                >
                  ▼ 展开全部 ({msg.content.length.toLocaleString()} chars)
                </button>
              </>
            ) : (
              msg.content
            )}
          </div>
        </>
      )}
    </div>
  );
}

/** Render a system event inline in the message timeline */
function EventBubble({ event }: { event: ConversationEvent }) {
  const {
    icon: Icon,
    color,
    label,
  } = (() => {
    const t = event.eventType;
    if (t === "turn_duration")
      return { icon: Timer, color: "text-neutral-500", label: "Turn" };
    if (t === "compact_boundary")
      return { icon: Layers, color: "text-yellow-500", label: "Context 压缩" };
    if (t.startsWith("queue:"))
      return {
        icon: Zap,
        color: "text-neutral-600",
        label: t.replace("queue:", "Queue: "),
      };
    if (t === "hook_progress")
      return { icon: Zap, color: "text-neutral-600", label: "Hook" };
    return { icon: Terminal, color: "text-neutral-600", label: t };
  })();

  return (
    <div className="flex items-center gap-2 py-0.5 opacity-50 hover:opacity-80 transition-opacity">
      <Icon className={cn("w-3 h-3 flex-shrink-0", color)} />
      <span className={cn("text-[10px] font-mono", color)}>{label}</span>
      {event.content && (
        <span className="text-[10px] text-neutral-600 truncate">
          {event.content}
        </span>
      )}
      <span className="text-[10px] text-neutral-700 ml-auto flex-shrink-0">
        {formatTime(event.timestamp)}
      </span>
    </div>
  );
}

/** Turn boundary separator — shows turn index, user content preview, tool stats */
function TurnHeaderBubble({ turn }: { turn: ConversationTurn }) {
  const tools = turn.toolNames?.split(",").filter(Boolean) || [];
  const duration = turn.startedAt && turn.endedAt
    ? ((new Date(turn.endedAt).getTime() - new Date(turn.startedAt).getTime()) / 1000)
    : null;

  return (
    <div className="flex items-stretch gap-0 my-2">
      {/* Left accent bar */}
      <div className="w-1 rounded-l bg-cyan-500/60 flex-shrink-0" />
      <div className="flex-1 border border-cyan-500/20 border-l-0 rounded-r bg-cyan-950/20 px-3 py-2">
        {/* Top row: turn index + user content preview */}
        <div className="flex items-center gap-2">
          <span className="text-[11px] font-bold text-cyan-400 flex-shrink-0">
            Turn {turn.turnIdx}
          </span>
          {turn.topic && (
            <span className="text-[11px] text-cyan-300/70 truncate max-w-[300px]">
              {turn.topic}
            </span>
          )}
          <span className="ml-auto text-[10px] text-neutral-600 flex-shrink-0">
            {turn.messageCount} msgs
            {turn.toolCallCount > 0 && ` · ${turn.toolCallCount} tools`}
            {duration != null && ` · ${duration >= 60 ? `${(duration / 60).toFixed(1)}m` : `${duration.toFixed(0)}s`}`}
          </span>
        </div>
        {/* User content preview */}
        {turn.userContent && (
          <p className="text-[11px] text-neutral-400 mt-1 truncate max-w-full">
            👤 {turn.userContent.slice(0, 120)}{turn.userContent.length > 120 ? "..." : ""}
          </p>
        )}
        {/* Bottom row: tool badges + flags */}
        {(tools.length > 0 || turn.hasCodeChange || turn.hasMcpCall) && (
          <div className="flex items-center gap-1.5 mt-1.5 flex-wrap">
            {tools.map((t, i) => (
              <span key={i} className="text-[9px] px-1.5 py-0.5 rounded bg-neutral-800 text-neutral-500 font-mono">
                {t}
              </span>
            ))}
            {turn.hasCodeChange && (
              <span className="text-[9px] px-1.5 py-0.5 rounded bg-amber-500/10 text-amber-400 font-mono">
                code-change
              </span>
            )}
            {turn.hasMcpCall && (
              <span className="text-[9px] px-1.5 py-0.5 rounded bg-purple-500/10 text-purple-400 font-mono">
                mcp
              </span>
            )}
          </div>
        )}
      </div>
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
  starred,
  onToggleStar,
}: {
  conv: Conversation;
  active: boolean;
  onClick: () => void;
  subagentCount?: number;
  expanded?: boolean;
  onToggleExpand?: () => void;
  isSubagent?: boolean;
  starred?: boolean;
  onToggleStar?: () => void;
}) {
  return (
    <div
      className={cn(isSubagent && "ml-4 border-l border-neutral-800/50 pl-1")}
    >
      <button
        onClick={onClick}
        className={cn(
          "w-full text-left p-3 rounded-lg border transition-colors",
          active
            ? "bg-neutral-800/50 border-orange-500/30"
            : starred
              ? "border-amber-500/30 hover:border-amber-500/50"
              : "border-neutral-800/50 hover:border-neutral-700",
          isSubagent && "py-2",
        )}
      >
        <div className="flex items-center justify-between mb-1">
          <div className="flex items-center gap-2 min-w-0">
            {onToggleStar && (
              <span
                onClick={(e) => { e.stopPropagation(); onToggleStar(); }}
                className={cn("cursor-pointer flex-shrink-0 text-[12px]", starred ? "text-amber-400" : "text-neutral-700 hover:text-neutral-500")}
                title={starred ? "取消标星" : "标星"}
              >
                {starred ? "★" : "☆"}
              </span>
            )}
            {isSubagent && (
              <GitBranch className="w-3 h-3 text-neutral-600 flex-shrink-0" />
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
                title={expanded ? "收起子任务" : "展开子任务"}
              >
                {expanded ? (
                  <ChevronDown className="w-3 h-3" />
                ) : (
                  <ChevronRight className="w-3 h-3" />
                )}
                <span>{subagentCount} 子任务</span>
              </button>
            ) : null}
            <Badge
              variant="outline"
              className={cn(
                "text-[10px] border-neutral-800",
                conv.status === "active"
                  ? "text-green-500"
                  : conv.status === "compacted"
                    ? "text-yellow-600"
                    : "text-neutral-600",
              )}
            >
              {conv.status === "active"
                ? "进行中"
                : conv.status === "compacted"
                  ? "已压缩"
                  : "已完成"}
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
              <span
                className="font-mono text-blue-400/60 truncate max-w-[80px]"
                title={conv.taskId}
              >
                {conv.taskId.slice(0, 8)}
              </span>
            )}
            {conv.slotId && (
              <span className="font-mono text-cyan-500/60">{conv.slotId}</span>
            )}
            {conv.source === "pty_jsonl" && (
              <span className="text-[9px] text-purple-500/50">PTY</span>
            )}
            {conv.model && (
              <span className="font-mono text-neutral-600 truncate max-w-[100px]">
                {conv.model}
              </span>
            )}
          </div>
          <div className="flex items-center gap-2 flex-shrink-0">
            {conv.endedAt && (
              <span className="text-[10px] text-neutral-600" title="持续时间">
                {(() => {
                  const ms =
                    new Date(conv.endedAt).getTime() -
                    new Date(conv.startedAt).getTime();
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

function GeminiListItem({
  conv,
  active,
  onClick,
  starred,
  onToggleStar,
}: {
  conv: Conversation;
  active: boolean;
  onClick: () => void;
  starred?: boolean;
  onToggleStar?: () => void;
}) {
  // Derive display label: taskId for router_chat, project name for gemini_cli
  const label = conv.taskId
    ? conv.taskId.slice(0, 8)
    : conv.project
      ? conv.project.split("/").filter(Boolean).pop() || "gemini"
      : "gemini";
  const sourceTag = conv.source === "gemini_cli" ? "CLI" : "Chat";
  return (
    <button
      onClick={onClick}
      className={cn(
        "w-full text-left px-3 py-2 rounded-md border transition-colors",
        active
          ? "bg-neutral-800/50 border-indigo-500/30"
          : starred
            ? "border-amber-500/30 hover:border-amber-500/50"
            : "border-neutral-800/30 hover:border-neutral-700",
      )}
    >
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2 min-w-0">
          {onToggleStar && (
            <span
              onClick={(e) => { e.stopPropagation(); onToggleStar(); }}
              className={cn("cursor-pointer flex-shrink-0 text-[12px]", starred ? "text-amber-400" : "text-neutral-700 hover:text-neutral-500")}
              title={starred ? "取消标星" : "标星"}
            >
              {starred ? "★" : "☆"}
            </span>
          )}
          <Sparkles className="w-3 h-3 text-indigo-400 flex-shrink-0" />
          <span className="text-[11px] font-mono text-indigo-300/80 truncate max-w-[120px]">
            {label}
          </span>
          <span className="text-[10px] font-mono text-neutral-600">
            {conv.model || "gemini"}
          </span>
          <span className="text-[9px] px-1 rounded bg-neutral-800 text-neutral-500">
            {sourceTag}
          </span>
        </div>
        <div className="flex items-center gap-2 flex-shrink-0">
          {conv.messageCount > 0 && (
            <span className="text-[10px] text-neutral-600">
              {conv.messageCount} 条
            </span>
          )}
          <span
            className={cn(
              "text-[10px]",
              conv.status === "active"
                ? "text-green-500/70"
                : "text-neutral-600",
            )}
          >
            {conv.status === "active" ? "进行中" : "已完成"}
          </span>
          <span className="text-[10px] text-neutral-600">
            {timeAgo(conv.startedAt)}
          </span>
        </div>
      </div>
    </button>
  );
}

export function Conversations() {
  const [conversations, setConversations] = useState<Conversation[]>([]);
  const [messages, setMessages] = useState<ConversationMessage[]>([]);
  const [events, setEvents] = useState<ConversationEvent[]>([]);
  const [labelsMap, setLabelsMap] = useState<
    Record<string, [string, string][]>
  >({});
  const [showLabels, setShowLabels] = useState(false);
  const [selectedId, setSelectedId] = useState<string | null>(() =>
    sessionStorage.getItem("conv:selectedId"),
  );
  const [jsonlPath, setJsonlPath] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadingMessages, setLoadingMessages] = useState(false);
  const [search, setSearch] = useState("");
  const [searchResults, setSearchResults] = useState<
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    any[] | null
  >(null);
  const [statusFilter, setStatusFilter] = useState<string | null>(null);
  const [viewMode, setViewMode] = useState<
    "conversations" | "workers" | "gemini"
  >(() => {
    const saved = sessionStorage.getItem("conv:viewMode");
    return saved === "conversations" ||
      saved === "workers" ||
      saved === "gemini" ||
      saved === "jarvis"
      ? saved
      : "conversations";
  });
  const [showList, setShowList] = useState(true); // mobile: toggle list/detail
  const [expandedParents, setExpandedParents] = useState<Set<string>>(
    new Set(),
  );
  const [collapsedDays, setCollapsedDays] = useState<Set<string>>(new Set());
  const [collapsedSlots, setCollapsedSlots] = useState<Set<string>>(new Set());
  const [starredIds, setStarredIds] = useState<Set<string>>(new Set());
  const toggleStar = useCallback((id: string) => {
    setStarredIds((prev) => {
      const next = new Set(prev);
      const isStarred = next.has(id);
      if (isStarred) next.delete(id); else next.add(id);
      // Persist to backend
      fetch('/api/conversations', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          action: isStarred ? 'delete_label' : 'set_label',
          sessionId: id,
          label: 'star',
        }),
      }).catch(() => {});
      return next;
    });
  }, []);
  const [userIndex, setUserIndex] = useState<{ id: number; time: string; preview: string }[]>([]);
  const [turns, setTurns] = useState<ConversationTurn[]>([]);
  const [hasMore, setHasMore] = useState(false); // whether more messages exist beyond loaded window
  const [loadingMore, setLoadingMore] = useState(false);
  // Scroll position persistence refs
  const listScrollRef = useRef<HTMLDivElement>(null);
  const restoredRef = useRef(false); // guard: only restore once after initial load

  // One-time migration: sync localStorage stars to backend, then clear
  useEffect(() => {
    try {
      const saved = localStorage.getItem("conv:starred");
      if (!saved) return;
      const ids: string[] = JSON.parse(saved);
      if (!Array.isArray(ids) || ids.length === 0) return;
      Promise.all(ids.map((id) =>
        fetch('/api/conversations', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ action: 'set_label', sessionId: id, label: 'star' }),
        }).catch(() => {}),
      )).then(() => localStorage.removeItem("conv:starred"));
    } catch { /* silent */ }
  }, []);

  // Persist selectedId & viewMode to sessionStorage
  useEffect(() => {
    if (selectedId) sessionStorage.setItem("conv:selectedId", selectedId);
    else sessionStorage.removeItem("conv:selectedId");
  }, [selectedId]);

  useEffect(() => {
    sessionStorage.setItem("conv:viewMode", viewMode);
  }, [viewMode]);

  // Track Virtuoso visible range for scroll persistence + minimap highlight
  const visibleRangeRef = useRef<{ startIndex: number }>({ startIndex: 0 });
  const [visibleStart, setVisibleStart] = useState(0);
  const pendingScrollToMsgId = useRef<number | null>(null);

  // Save scroll positions before unload
  useEffect(() => {
    const save = () => {
      if (listScrollRef.current)
        sessionStorage.setItem(
          "conv:listScroll",
          String(listScrollRef.current.scrollTop),
        );
      sessionStorage.setItem(
        "conv:msgScrollIdx",
        String(visibleRangeRef.current.startIndex),
      );
    };
    window.addEventListener("beforeunload", save);
    return () => window.removeEventListener("beforeunload", save);
  }, []);

  const isGeminiSource = useCallback((source: string) => {
    return source === "router_chat" || source === "gemini_cli";
  }, []);

  const fetchConversations = useCallback(async () => {
    setLoading(true);
    try {
      // Server-side source filtering per tab. No conversationType filter —
      // frontend is a faithful DB viewer, misclassified records must be visible.
      const params = new URLSearchParams();
      if (statusFilter) params.set("status", statusFilter);
      params.set("limit", "300");
      params.set("conversationType", "all");

      if (viewMode === "gemini") {
        params.set("conversationType", "gemini");
      } else if (viewMode === "jarvis") {
        params.set("conversationType", "jarvis");
      } else if (viewMode === "workers") {
        params.set("conversationType", "system");
      } else {
        params.set("conversationType", "user");
      }

      const res = await fetch(`/api/conversations?${params}`);
      if (res.ok) {
        const data = await res.json();
        const list = Array.isArray(data) ? data : [];
        setConversations(list);
        // Extract starred IDs from conversation labels
        const stars = new Set<string>();
        for (const c of list) {
          if (c.labels && Array.isArray(c.labels)) {
            for (const [label] of c.labels) {
              if (label === "star") { stars.add(c.id); break; }
            }
          }
        }
        setStarredIds(stars);
      }
    } catch {
      // silent
    }
    setLoading(false);
  }, [statusFilter, viewMode]);

  const PAGE_SIZE = 500;

  const fetchMessages = useCallback(
    async (sessionId: string, withLabels?: boolean) => {
      setLoadingMessages(true);
      setSearchResults(null);
      try {
        const labelsParam = withLabels ? "&labels=1" : "";
        // Load from beginning (sinceId=0) instead of tail
        const res = await fetch(
          `/api/conversations?sessionId=${encodeURIComponent(sessionId)}&sinceId=0&tail=${PAGE_SIZE}${labelsParam}`,
        );
        if (res.ok) {
          const data = await res.json();
          const msgs = data.messages || [];
          setMessages(msgs);
          setEvents(data.events || []);
          setJsonlPath(data.conversation?.jsonlPath || null);
          setLabelsMap(data.labels || {});
          setUserIndex(data.userIndex || []);
          setTurns(data.turns || []);
          setHasMore(msgs.length >= PAGE_SIZE);
        }
      } catch {
        setMessages([]);
        setEvents([]);
        setJsonlPath(null);
        setLabelsMap({});
        setTurns([]);
        setHasMore(false);
      }
      setLoadingMessages(false);
    },
    [],
  );

  const loadMoreMessages = useCallback(async () => {
    if (!selectedId || loadingMore || !hasMore || messages.length === 0) return;
    setLoadingMore(true);
    try {
      const lastId = messages[messages.length - 1].id;
      const res = await fetch(
        `/api/conversations?sessionId=${encodeURIComponent(selectedId)}&sinceId=${lastId}&tail=${PAGE_SIZE}`,
      );
      if (res.ok) {
        const data = await res.json();
        const newMsgs: ConversationMessage[] = data.messages || [];
        if (newMsgs.length > 0) {
          setMessages((prev) => [...prev, ...newMsgs]);
        }
        setHasMore(newMsgs.length >= PAGE_SIZE);
      }
    } catch {
      /* ignore */
    }
    setLoadingMore(false);
  }, [selectedId, loadingMore, hasMore, messages]);

  const handleSearch = useCallback(async () => {
    if (!search.trim()) {
      setSearchResults(null);
      return;
    }
    setLoading(true);
    try {
      const ctParam = viewMode === "gemini" ? "&conversationType=gemini"
        : viewMode === "jarvis" ? "&conversationType=jarvis"
        : viewMode === "workers" ? "&conversationType=system"
        : "&conversationType=user";
      const res = await fetch(
        `/api/conversations?search=${encodeURIComponent(search)}&limit=50${ctParam}`,
      );
      if (res.ok) {
        const data = await res.json();
        setSearchResults(data.results || []);
      }
    } catch {
      setSearchResults([]);
    }
    setLoading(false);
  }, [search, viewMode]);

  useEffect(() => {
    fetchConversations();
  }, [fetchConversations]);

  // Restore saved conversation selection after conversations load
  useEffect(() => {
    if (restoredRef.current || loading || conversations.length === 0) return;
    restoredRef.current = true;
    const savedId = sessionStorage.getItem("conv:selectedId");
    if (savedId && conversations.some((c) => c.id === savedId)) {
      setSelectedId(savedId);
      fetchMessages(savedId, showLabels);
      // Restore list scroll position after DOM updates
      requestAnimationFrame(() => {
        const savedListScroll = sessionStorage.getItem("conv:listScroll");
        if (savedListScroll && listScrollRef.current) {
          listScrollRef.current.scrollTop = Number(savedListScroll);
        }
      });
    }
  }, [loading, conversations, fetchMessages, showLabels]);

  const selectConversation = useCallback(
    (id: string) => {
      setSelectedId(id);
      setShowList(false);
      fetchMessages(id, showLabels);
    },
    [fetchMessages, showLabels],
  );

  const selectedConv = useMemo(
    () => conversations.find((c) => c.id === selectedId),
    [conversations, selectedId],
  );

  // Flatten messages and events into a single timeline array for virtual scrolling
  type FlatItem =
    | { type: "date-header"; date: string }
    | { type: "message"; data: ConversationMessage }
    | { type: "event"; data: ConversationEvent }
    | { type: "turn-header"; turn: ConversationTurn }
    | {
        type: "tool-pair";
        call: ConversationMessage;
        result: ConversationMessage;
      };

  const flatTimeline = useMemo(() => {
    const earliestMsg =
      messages.length > 0
        ? new Date(messages[0].timestamp).getTime()
        : Infinity;

    const importantEvents = events.filter((e) => {
      const t = e.eventType;
      if (
        !(
          t === "turn_duration" ||
          t === "compact_boundary" ||
          t.startsWith("queue:")
        )
      )
        return false;
      return new Date(e.timestamp).getTime() >= earliestMsg;
    });

    // Build a map: startMessageId → turn (for inserting turn headers)
    const turnByStartMsg = new Map<number, ConversationTurn>();
    for (const t of turns) {
      turnByStartMsg.set(t.startMessageId, t);
    }

    // Sort messages + events by timestamp
    type SortedItem =
      | { type: "message"; data: ConversationMessage }
      | { type: "event"; data: ConversationEvent };
    const sorted: SortedItem[] = [
      ...messages.map((m) => ({ type: "message" as const, data: m })),
      ...importantEvents.map((e) => ({ type: "event" as const, data: e })),
    ].sort(
      (a, b) =>
        new Date(a.data.timestamp).getTime() -
        new Date(b.data.timestamp).getTime(),
    );

    // Merge consecutive tool_use + tool_result into tool-pair items
    const flat: FlatItem[] = [];
    let currentDate = "";
    let i = 0;
    while (i < sorted.length) {
      const item = sorted[i];
      const date = formatDate(item.data.timestamp);
      if (date !== currentDate) {
        currentDate = date;
        flat.push({ type: "date-header", date });
      }

      // Insert turn-header before the turn's start message
      if (item.type === "message") {
        const turn = turnByStartMsg.get(item.data.id);
        if (turn) {
          flat.push({ type: "turn-header", turn });
        }
      }

      // Check for tool_use(assistant) + tool_result pair
      if (
        item.type === "message" &&
        item.data.toolName &&
        item.data.role === "assistant"
      ) {
        const next = sorted[i + 1];
        if (next?.type === "message" && next.data.role === "tool_result") {
          flat.push({ type: "tool-pair", call: item.data, result: next.data });
          i += 2;
          continue;
        }
      }
      flat.push(item);
      i++;
    }
    return flat;
  }, [messages, events, turns]);

  const virtuosoRef = useRef<VirtuosoHandle>(null);

  // Restore message scroll position after messages finish loading (Virtuoso)
  useEffect(() => {
    if (!loadingMessages && flatTimeline.length > 0) {
      const savedIdx = sessionStorage.getItem("conv:msgScrollIdx");
      if (savedIdx && virtuosoRef.current) {
        requestAnimationFrame(() => {
          virtuosoRef.current?.scrollToIndex({
            index: Number(savedIdx),
            align: "start",
          });
        });
        sessionStorage.removeItem("conv:msgScrollIdx");
      }
    }
  }, [loadingMessages, flatTimeline]);

  // Handle pending scroll to a message ID (after onLoadAround reloads messages)
  useEffect(() => {
    const targetId = pendingScrollToMsgId.current;
    if (targetId == null || flatTimeline.length === 0) return;
    const idx = flatTimeline.findIndex(
      (it: { type: string; data?: { id: number } }) =>
        it.type === "message" && it.data?.id === targetId,
    );
    if (idx >= 0) {
      pendingScrollToMsgId.current = null;
      requestAnimationFrame(() => {
        virtuosoRef.current?.scrollToIndex({ index: idx, align: "start" });
      });
    }
  }, [flatTimeline]);

  const filterByTab = useCallback(
    (c: Conversation, tab: typeof viewMode) => {
      if (tab === "conversations") {
        return c.conversationType === "user" && !isGeminiSource(c.source);
      }
      if (tab === "gemini") {
        return isGeminiSource(c.source);
      }
      // workers: catch-all for non-user non-gemini
      return c.conversationType !== "user" && !isGeminiSource(c.source);
    },
    [isGeminiSource],
  );

  const counts = useMemo(() => {
    const filtered = conversations.filter((c) => filterByTab(c, viewMode));
    const active = filtered.filter((c) => c.status === "active").length;
    const completed = filtered.filter((c) => c.status === "completed").length;
    const compacted = filtered.filter((c) => c.status === "compacted").length;
    return { active, completed, compacted, total: filtered.length };
  }, [conversations, viewMode, filterByTab]);

  // Tab counts: current tab shows exact count, others show '…' until switched
  const tabCounts = useMemo(
    () => ({
      conversations: viewMode === "conversations" ? conversations.length : null,
      jarvis: viewMode === "jarvis" ? conversations.length : null,
      workers: viewMode === "workers" ? conversations.length : null,
      gemini: viewMode === "gemini" ? conversations.length : null,
    }),
    [conversations, viewMode],
  );

  // Group: separate subagents and compacted sessions from main list.
  // Data is already tab-filtered from the API, no client-side filterByTab needed.
  const { mainList, subagentMap } = useMemo(() => {
    const map = new Map<string, Conversation[]>();
    const main: Conversation[] = [];
    for (const conv of conversations) {
      // Only fold subagent/compaction types into parent map.
      // Compaction *continuations* (user/worker with parentSessionId) stay in main list
      // so they remain visible — parentSessionId just records the stitching lineage.
      const isSubordinateType =
        conv.conversationType === "subagent" ||
        conv.conversationType === "compaction";
      if (conv.parentSessionId && isSubordinateType) {
        const list = map.get(conv.parentSessionId) || [];
        list.push(conv);
        map.set(conv.parentSessionId, list);
      } else if (conv.status === "compacted") {
        continue;
      } else {
        main.push(conv);
      }
    }
    // Sort: active first, then by most recent
    main.sort((a, b) => {
      if (a.status === "active" && b.status !== "active") return -1;
      if (a.status !== "active" && b.status === "active") return 1;
      return new Date(b.startedAt).getTime() - new Date(a.startedAt).getTime();
    });
    return { mainList: main, subagentMap: map };
  }, [conversations]);

  const dayGroups = useMemo(() => groupByDay(mainList), [mainList]);

  // Starred conversations (shown at top of list)
  const starredConvs = useMemo(
    () => mainList.filter((c) => starredIds.has(c.id)),
    [mainList, starredIds],
  );

  // Workers tab: group sessions by slotId
  const slotGroups = useMemo(() => {
    if (viewMode !== "workers") return [];
    const map = new Map<string, Conversation[]>();
    for (const conv of mainList) {
      const key = conv.slotId || "_unassigned";
      const arr = map.get(key) || [];
      arr.push(conv);
      map.set(key, arr);
    }
    // Build sorted group list
    const groups = Array.from(map.entries()).map(([slotId, sessions]) => {
      const activeCount = sessions.filter((s) => s.status === "active").length;
      const latestAt = sessions[0]?.startedAt || "";
      return {
        slotId,
        sessions,
        activeCount,
        totalCount: sessions.length,
        latestAt,
      };
    });
    // Sort: active slots first, then by most recent session
    groups.sort((a, b) => {
      if (a.activeCount > 0 && b.activeCount === 0) return -1;
      if (a.activeCount === 0 && b.activeCount > 0) return 1;
      return new Date(b.latestAt).getTime() - new Date(a.latestAt).getTime();
    });
    return groups;
  }, [mainList, viewMode]);

  const toggleSlotCollapse = useCallback((slotId: string) => {
    setCollapsedSlots((prev) => {
      const next = new Set(prev);
      if (next.has(slotId)) next.delete(slotId);
      else next.add(slotId);
      return next;
    });
  }, []);

  // Compute the visible (non-collapsed) conversation list in screen order for keyboard nav
  const visibleList = useMemo(() => {
    if (viewMode === "workers") {
      const list: Conversation[] = [];
      for (const { slotId, sessions } of slotGroups) {
        if (!collapsedSlots.has(slotId)) {
          list.push(...sessions);
        }
      }
      return list;
    }
    // For conversations/gemini tabs, mainList order matches screen order
    return mainList;
  }, [viewMode, slotGroups, collapsedSlots, mainList]);

  const toggleDayCollapse = useCallback((dayKey: string) => {
    setCollapsedDays((prev) => {
      const next = new Set(prev);
      if (next.has(dayKey)) next.delete(dayKey);
      else next.add(dayKey);
      return next;
    });
  }, []);

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

  const isAllCollapsed = useMemo(() => {
    if (viewMode === "workers") {
      return (
        slotGroups.length > 0 &&
        slotGroups.every((g) => collapsedSlots.has(g.slotId))
      );
    } else {
      return (
        dayGroups.length > 0 &&
        dayGroups.every((g) => collapsedDays.has(g.dayKey))
      );
    }
  }, [viewMode, slotGroups, dayGroups, collapsedSlots, collapsedDays]);

  const toggleCollapseAll = useCallback(() => {
    if (isAllCollapsed) {
      if (viewMode === "workers") setCollapsedSlots(new Set());
      else setCollapsedDays(new Set());
    } else {
      if (viewMode === "workers")
        setCollapsedSlots(new Set(slotGroups.map((g) => g.slotId)));
      else setCollapsedDays(new Set(dayGroups.map((g) => g.dayKey)));
    }
  }, [isAllCollapsed, viewMode, slotGroups, dayGroups]);

  return (
    <div className="flex-1 flex min-h-0 overflow-hidden">
      {/* Left: Conversation list */}
      <div
        className={cn(
          "w-80 flex-shrink-0 border-r border-neutral-800 flex flex-col",
          !showList && "hidden md:flex",
        )}
      >
        {/* View mode tabs */}
        <div className="flex border-b border-neutral-800">
          <button
            onClick={() => setViewMode("conversations")}
            className={cn(
              "flex-1 py-2 text-xs font-medium transition-colors flex items-center justify-center gap-1.5",
              viewMode === "conversations"
                ? "text-neutral-200 border-b-2 border-blue-500"
                : "text-neutral-500 hover:text-neutral-400",
            )}
          >
            <MessageSquare className="w-3 h-3" />
            对话
          </button>
          <button
            onClick={() => setViewMode("jarvis")}
            className={cn(
              "flex-1 py-2 text-xs font-medium transition-colors flex items-center justify-center gap-1.5",
              viewMode === "jarvis"
                ? "text-neutral-200 border-b-2 border-purple-500"
                : "text-neutral-500 hover:text-neutral-400",
            )}
          >
            <Zap className="w-3 h-3" />
            Jarvis
          </button>
          <button
            onClick={() => setViewMode("workers")}
            className={cn(
              "flex-1 py-2 text-xs font-medium transition-colors flex items-center justify-center gap-1.5",
              viewMode === "workers"
                ? "text-neutral-200 border-b-2 border-orange-500"
                : "text-neutral-500 hover:text-neutral-400",
            )}
          >
            <Server className="w-3 h-3" />
            后台
          </button>
          <button
            onClick={() => setViewMode("gemini")}
            className={cn(
              "flex-1 py-2 text-xs font-medium transition-colors flex items-center justify-center gap-1.5",
              viewMode === "gemini"
                ? "text-neutral-200 border-b-2 border-indigo-500"
                : "text-neutral-500 hover:text-neutral-400",
            )}
          >
            <Sparkles className="w-3 h-3" />
            Gemini
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
              onKeyDown={(e) => e.key === "Enter" && handleSearch()}
              className="w-full pl-9 pr-3 py-1.5 bg-neutral-900 border border-neutral-800 rounded-md text-xs text-neutral-300 placeholder:text-neutral-600 focus:outline-none focus:border-neutral-700"
            />
          </div>

          {/* Filters */}
          <div className="flex items-center gap-1.5">
            <button
              onClick={() => setStatusFilter(null)}
              className={cn(
                "px-2 py-0.5 text-[10px] rounded-full border transition-colors",
                !statusFilter
                  ? "bg-neutral-800 text-white border-neutral-700"
                  : "text-neutral-500 border-neutral-800 hover:text-neutral-300",
              )}
            >
              全部 {counts.total}
            </button>
            <button
              onClick={() => setStatusFilter("active")}
              className={cn(
                "px-2 py-0.5 text-[10px] rounded-full border transition-colors",
                statusFilter === "active"
                  ? "bg-green-500/10 text-green-400 border-green-500/30"
                  : "text-neutral-500 border-neutral-800 hover:text-neutral-300",
              )}
            >
              进行中 {counts.active}
            </button>
            <button
              onClick={() => setStatusFilter("completed")}
              className={cn(
                "px-2 py-0.5 text-[10px] rounded-full border transition-colors",
                statusFilter === "completed"
                  ? "bg-neutral-700 text-neutral-300 border-neutral-600"
                  : "text-neutral-500 border-neutral-800 hover:text-neutral-300",
              )}
            >
              已完成 {counts.completed}
            </button>
            <button
              onClick={fetchConversations}
              className="ml-auto p-1 rounded text-neutral-600 hover:text-neutral-400 transition-colors"
              title="刷新"
            >
              <RefreshCw className={cn("w-3 h-3", loading && "animate-spin")} />
            </button>
            <button
              onClick={toggleCollapseAll}
              className="p-1 rounded text-neutral-600 hover:text-neutral-400 transition-colors"
              title={isAllCollapsed ? "展开全部" : "折叠全部"}
            >
              {isAllCollapsed ? (
                <ChevronsUpDown className="w-3 h-3" />
              ) : (
                <ChevronsDownUp className="w-3 h-3" />
              )}
            </button>
          </div>
        </div>

        {/* Search results */}
        {searchResults !== null ? (
          <div className="flex-1 overflow-auto p-2 space-y-1">
            <div className="flex items-center justify-between px-1 mb-2">
              <span className="text-[11px] text-neutral-500">
                搜索到 {searchResults.length} 条会话
              </span>
              <button
                onClick={() => {
                  setSearch("");
                  setSearchResults(null);
                }}
                className="text-[11px] text-neutral-600 hover:text-neutral-400"
              >
                清除
              </button>
            </div>
            {searchResults.map((r) => (
              <button
                key={r.sessionId || r.id}
                onClick={() => selectConversation(r.sessionId || r.id)}
                className="w-full text-left p-2 rounded-md border border-neutral-800/50 hover:border-neutral-700 transition-colors"
              >
                <div className="flex items-center gap-2 mb-0.5">
                  <span className="text-[10px] font-mono text-indigo-300/70 truncate max-w-[140px]">
                    {r.sessionId?.slice(0, 8) || r.id}
                  </span>
                  {r.source && (
                    <span className="text-[9px] px-1 rounded bg-neutral-800 text-neutral-500">{r.source}</span>
                  )}
                  {r.messageCount != null && (
                    <span className="text-[10px] text-neutral-600">{r.messageCount} 条</span>
                  )}
                  <span className="text-[10px] text-neutral-600">
                    {timeAgo(r.startedAt || r.timestamp)}
                  </span>
                </div>
                {r.summary && (
                  <p className="text-[11px] text-neutral-400 truncate mb-0.5">{r.summary}</p>
                )}
                <p className="text-xs text-neutral-500 line-clamp-2">
                  {r.matchReason || r.content}
                </p>
              </button>
            ))}
          </div>
        ) : (
          /* Conversation list — arrow key navigation */
          <div
            ref={listScrollRef}
            className="flex-1 overflow-auto p-2 space-y-1"
            tabIndex={0}
            onKeyDown={(e) => {
              if (
                !["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight"].includes(
                  e.key,
                )
              )
                return;
              e.preventDefault();

              const currentConv = visibleList.find((c) => c.id === selectedId);

              if (e.key === "ArrowRight") {
                if (
                  currentConv &&
                  subagentMap.has(currentConv.id) &&
                  !expandedParents.has(currentConv.id)
                ) {
                  toggleParentExpand(currentConv.id);
                }
                return;
              }

              if (e.key === "ArrowLeft") {
                if (currentConv) {
                  // 1. If it's a parent and is expanded, collapse it
                  if (
                    subagentMap.has(currentConv.id) &&
                    expandedParents.has(currentConv.id)
                  ) {
                    toggleParentExpand(currentConv.id);
                    return;
                  }
                  // 2. If it's a subagent, jump to its parent
                  if (
                    currentConv.parentSessionId &&
                    expandedParents.has(currentConv.parentSessionId)
                  ) {
                    selectConversation(currentConv.parentSessionId);
                    const el = document.getElementById(
                      `conv-${currentConv.parentSessionId}`,
                    );
                    el?.scrollIntoView({ block: "nearest" });
                    return;
                  }
                  // 3. Otherwise, collapse the day/slot group it belongs to
                  if (viewMode === "workers") {
                    const slotKey = currentConv.slotId || "_unassigned";
                    if (!collapsedSlots.has(slotKey))
                      toggleSlotCollapse(slotKey);
                  } else {
                    const dayKey = getDayKey(currentConv.startedAt);
                    if (!collapsedDays.has(dayKey)) toggleDayCollapse(dayKey);
                  }
                }
                return;
              }

              const idx = visibleList.findIndex((c) => c.id === selectedId);
              const next =
                e.key === "ArrowDown"
                  ? Math.min(idx + 1, visibleList.length - 1)
                  : Math.max(idx - 1, 0);
              if (next !== idx && visibleList[next]) {
                selectConversation(visibleList[next].id);
                const el = document.getElementById(
                  `conv-${visibleList[next].id}`,
                );
                el?.scrollIntoView({ block: "nearest" });
              }
            }}
          >
            {loading && conversations.length === 0 ? (
              <div className="text-center py-8 text-neutral-600 text-xs">
                加载中...
              </div>
            ) : mainList.length === 0 ? (
              <div className="text-center py-8 text-neutral-600 text-xs">
                {viewMode === "conversations"
                  ? "暂无对话记录"
                  : viewMode === "jarvis"
                    ? "暂无 Jarvis 会话"
                    : viewMode === "workers"
                      ? "暂无后台工位会话"
                      : "暂无 Gemini 会话"}
              </div>
            ) : viewMode === "workers" ? (
              /* Workers tab: group by slot */
              slotGroups.map(
                ({ slotId, sessions, activeCount, totalCount }) => {
                  const isSlotCollapsed = collapsedSlots.has(slotId);
                  const displayName =
                    slotId === "_unassigned" ? "未绑定工位" : slotId;
                  return (
                    <div key={slotId}>
                      <button
                        onClick={() => toggleSlotCollapse(slotId)}
                        className="w-full flex items-center gap-1.5 px-3 py-1.5 text-[11px] text-neutral-500 hover:text-neutral-300 sticky top-0 bg-neutral-950/90 backdrop-blur-sm z-10 border-b border-neutral-800/50"
                      >
                        {isSlotCollapsed ? (
                          <ChevronRight className="w-3 h-3" />
                        ) : (
                          <ChevronDown className="w-3 h-3" />
                        )}
                        <span
                          className={cn(
                            "font-mono font-medium",
                            activeCount > 0
                              ? "text-green-400/80"
                              : "text-neutral-500",
                          )}
                        >
                          {displayName}
                        </span>
                        {activeCount > 0 && (
                          <span className="text-[9px] px-1 py-0.5 rounded bg-green-500/20 text-green-400">
                            运行中 {activeCount}
                          </span>
                        )}
                        <span className="text-neutral-600 ml-auto">
                          {totalCount}
                        </span>
                      </button>
                      {!isSlotCollapsed &&
                        sessions.map((conv) => {
                          const children = subagentMap.get(conv.id) || [];
                          const isExpanded = expandedParents.has(conv.id);
                          return (
                            <div key={conv.id} id={`conv-${conv.id}`}>
                              <ConversationListItem
                                conv={conv}
                                active={conv.id === selectedId}
                                onClick={() => selectConversation(conv.id)}
                                subagentCount={children.length}
                                expanded={isExpanded}
                                onToggleExpand={() =>
                                  toggleParentExpand(conv.id)
                                }
                                starred={starredIds.has(conv.id)}
                                onToggleStar={() => toggleStar(conv.id)}
                              />
                              {isExpanded &&
                                children.map((child) => (
                                  <ConversationListItem
                                    key={child.id}
                                    conv={child}
                                    active={child.id === selectedId}
                                    onClick={() => selectConversation(child.id)}
                                    isSubagent
                                    starred={starredIds.has(child.id)}
                                    onToggleStar={() => toggleStar(child.id)}
                                  />
                                ))}
                            </div>
                          );
                        })}
                    </div>
                  );
                },
              )
            ) : (
              /* Conversations + Gemini tabs: starred + group by day */
              <>
              {starredConvs.length > 0 && (
                <div className="mb-2">
                  <div className="flex items-center gap-2 px-2 py-1.5 text-[11px] text-amber-400 font-medium">
                    <span>★ 标星</span>
                    <span className="text-neutral-600">{starredConvs.length}</span>
                  </div>
                  <div className="space-y-0.5">
                    {starredConvs.map((conv) =>
                      viewMode === "gemini" ? (
                        <div key={`star-${conv.id}`} id={`conv-${conv.id}`}>
                          <GeminiListItem
                            conv={conv}
                            active={conv.id === selectedId}
                            onClick={() => selectConversation(conv.id)}
                            starred
                            onToggleStar={() => toggleStar(conv.id)}
                          />
                        </div>
                      ) : (
                        <div key={`star-${conv.id}`} id={`conv-${conv.id}`}>
                          <ConversationListItem
                            conv={conv}
                            active={conv.id === selectedId}
                            onClick={() => selectConversation(conv.id)}
                            starred
                            onToggleStar={() => toggleStar(conv.id)}
                          />
                        </div>
                      ),
                    )}
                  </div>
                </div>
              )}
              {dayGroups.map(({ dayKey, label, items }) => {
                const isDayCollapsed = collapsedDays.has(dayKey);
                return (
                  <div key={dayKey}>
                    <button
                      onClick={() => toggleDayCollapse(dayKey)}
                      className="w-full flex items-center gap-1.5 px-3 py-1.5 text-[11px] text-neutral-500 hover:text-neutral-300 sticky top-0 bg-neutral-950/90 backdrop-blur-sm z-10 border-b border-neutral-800/50"
                    >
                      {isDayCollapsed ? (
                        <ChevronRight className="w-3 h-3" />
                      ) : (
                        <ChevronDown className="w-3 h-3" />
                      )}
                      <span className="font-medium">{label}</span>
                      <span className="text-neutral-600 ml-auto">
                        {items.length}
                      </span>
                    </button>
                    {!isDayCollapsed &&
                      (viewMode === "gemini" ? (
                        <div className="space-y-0.5">
                          {items.map((conv) => (
                            <div key={conv.id} id={`conv-${conv.id}`}>
                              <GeminiListItem
                                conv={conv}
                                active={conv.id === selectedId}
                                onClick={() => selectConversation(conv.id)}
                                starred={starredIds.has(conv.id)}
                                onToggleStar={() => toggleStar(conv.id)}
                              />
                            </div>
                          ))}
                        </div>
                      ) : (
                        items.map((conv) => {
                          const children = subagentMap.get(conv.id) || [];
                          const isExpanded = expandedParents.has(conv.id);
                          return (
                            <div key={conv.id} id={`conv-${conv.id}`}>
                              <ConversationListItem
                                conv={conv}
                                active={conv.id === selectedId}
                                onClick={() => selectConversation(conv.id)}
                                subagentCount={children.length}
                                expanded={isExpanded}
                                onToggleExpand={() =>
                                  toggleParentExpand(conv.id)
                                }
                                starred={starredIds.has(conv.id)}
                                onToggleStar={() => toggleStar(conv.id)}
                              />
                              {isExpanded &&
                                children.map((child) => (
                                  <ConversationListItem
                                    key={child.id}
                                    conv={child}
                                    active={child.id === selectedId}
                                    onClick={() => selectConversation(child.id)}
                                    isSubagent
                                    starred={starredIds.has(child.id)}
                                    onToggleStar={() => toggleStar(child.id)}
                                  />
                                ))}
                            </div>
                          );
                        })
                      ))}
                  </div>
                );
              })}
              </>
            )}
          </div>
        )}
      </div>

      {/* Right: Message detail */}
      <div
        className={cn(
          "flex-1 flex flex-col min-w-0",
          showList && "hidden md:flex",
        )}
      >
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
                      onClick={() =>
                        selectConversation(selectedConv.parentSessionId!)
                      }
                      className="flex items-center gap-1 text-[11px] text-neutral-500 hover:text-neutral-300 transition-colors"
                      title="返回父会话"
                    >
                      <GitBranch className="w-3 h-3" />
                      <span>子任务</span>
                    </button>
                  )}
                  {selectedConv.project && (
                    <span className="text-sm font-medium text-neutral-200">
                      {selectedConv.project.split("/").pop()}
                    </span>
                  )}
                  <Badge
                    variant="outline"
                    className={cn(
                      "text-[10px] border-neutral-800",
                      selectedConv.status === "active"
                        ? "text-green-500"
                        : selectedConv.status === "compacted"
                          ? "text-yellow-600"
                          : "text-neutral-600",
                    )}
                  >
                    {selectedConv.status === "active"
                      ? "进行中"
                      : selectedConv.status === "compacted"
                        ? "已压缩"
                        : "已完成"}
                  </Badge>
                </div>
                <div className="flex items-center gap-3 text-[11px] text-neutral-500">
                  {selectedConv.messageCount > 0 && (
                    <span>{selectedConv.messageCount} 条消息</span>
                  )}
                  {selectedConv.model && (
                    <span className="font-mono">{selectedConv.model}</span>
                  )}
                  {selectedConv.slotId && (
                    <span className="font-mono text-cyan-500/60">
                      {selectedConv.slotId}
                    </span>
                  )}
                  <span>
                    {new Date(selectedConv.startedAt).toLocaleString("zh-CN")}
                  </span>
                </div>
                {selectedConv.llmSummary && (
                  <p className="text-[11px] text-neutral-500 mt-0.5 line-clamp-1">
                    {selectedConv.llmSummary}
                  </p>
                )}
              </div>
              <button
                onClick={() => {
                  const next = !showLabels;
                  setShowLabels(next);
                  if (selectedId) fetchMessages(selectedId, next);
                }}
                className={cn(
                  "flex items-center gap-1 px-2 py-1 rounded-md text-[11px] font-medium transition-colors flex-shrink-0",
                  showLabels
                    ? "bg-amber-500/10 text-amber-300 border border-amber-500/30"
                    : "text-neutral-500 hover:text-neutral-300 border border-neutral-800 hover:border-neutral-700",
                )}
                title="显示/隐藏标签"
              >
                <Tag className="w-3 h-3" />
                标签
              </button>
            </div>

            {/* Messages + Events Timeline */}
            <div className="flex-1 overflow-hidden px-4 py-2 flex flex-col">
              {loadingMessages ? (
                <div className="text-center py-8 text-neutral-600 text-xs">
                  加载消息...
                </div>
              ) : messages.length === 0 &&
                selectedConv.source === "router_chat" ? (
                <div className="flex items-center justify-center py-12">
                  <div className="max-w-sm text-center space-y-3">
                    <Sparkles className="w-8 h-8 text-indigo-400/50 mx-auto" />
                    <div className="text-sm text-neutral-400 font-medium">
                      Gemini Router Chat
                    </div>
                    <div className="text-xs text-neutral-600 space-y-1">
                      {selectedConv.taskId && (
                        <div>
                          Task:{" "}
                          <span className="font-mono text-indigo-300/70">
                            {selectedConv.taskId}
                          </span>
                        </div>
                      )}
                      <div>
                        Model:{" "}
                        <span className="font-mono">
                          {selectedConv.model || "gemini"}
                        </span>
                      </div>
                      <div>
                        {new Date(selectedConv.startedAt).toLocaleString(
                          "zh-CN",
                        )}
                      </div>
                    </div>
                    <p className="text-[11px] text-neutral-600 border border-neutral-800 rounded px-3 py-2">
                      消息已归档或通过滚动摘要压缩。
                    </p>
                    {selectedConv.taskId && (
                      <button
                        onClick={() => {
                          // Navigate to Board tab with this task
                          const boardTab = document.querySelector(
                            '[data-tab="board"]',
                          ) as HTMLElement;
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
                <div className="text-center py-8 text-neutral-600 text-xs">
                  暂无消息
                </div>
              ) : (
                <>
                  {/* Event stats summary */}
                  {(events.length > 0 || turns.length > 0) && (
                    <div className="flex items-center gap-3 px-1 py-1.5 mb-2 text-[10px] text-neutral-600 border border-neutral-800/50 rounded">
                      <Layers className="w-3 h-3" />
                      {events.length > 0 && <span>{events.length} 系统事件</span>}
                      {turns.length > 0 && (
                        <span className="text-cyan-500/70">
                          {turns.length} turns (S3分割)
                          {(() => {
                            const codeChanges = turns.filter(t => t.hasCodeChange).length;
                            const mcpCalls = turns.filter(t => t.hasMcpCall).length;
                            const parts: string[] = [];
                            if (codeChanges > 0) parts.push(`${codeChanges} 含代码修改`);
                            if (mcpCalls > 0) parts.push(`${mcpCalls} 含MCP`);
                            return parts.length > 0 ? ` · ${parts.join(" · ")}` : "";
                          })()}
                        </span>
                      )}
                      {(() => {
                        const turnEvents = events.filter(
                          (e) => e.eventType === "turn_duration",
                        );
                        if (turnEvents.length === 0) return null;
                        const totalMs = turnEvents.reduce(
                          (sum, e) =>
                            sum +
                            (parseInt(e.content?.replace("ms", "") || "0") ||
                              0),
                          0,
                        );
                        return (
                          <span>
                            {turnEvents.length} turn_duration, 总计{" "}
                            {(totalMs / 1000).toFixed(1)}s
                          </span>
                        );
                      })()}
                      {(() => {
                        const compacts = events.filter(
                          (e) => e.eventType === "compact_boundary",
                        ).length;
                        return compacts > 0 ? (
                          <span>{compacts} 次压缩</span>
                        ) : null;
                      })()}
                    </div>
                  )}
                  {/* Label stats summary */}
                  {showLabels &&
                    Object.keys(labelsMap).length > 0 &&
                    (() => {
                      const counts: Record<string, number> = {};
                      for (const pairs of Object.values(labelsMap)) {
                        for (const [label] of pairs) {
                          counts[label] = (counts[label] || 0) + 1;
                        }
                      }
                      return (
                        <div className="flex items-center gap-2 px-1 py-1.5 mb-2 text-[10px] border border-amber-500/20 bg-amber-500/5 rounded flex-wrap">
                          <Tag className="w-3 h-3 text-amber-400 flex-shrink-0" />
                          <span className="text-amber-300">
                            {Object.keys(labelsMap).length} 条消息有标签
                          </span>
                          {Object.entries(counts)
                            .sort((a, b) => b[1] - a[1])
                            .map(([label, count]) => {
                              const style = LABEL_STYLES[label] || {
                                text: "text-neutral-400",
                                bg: "bg-neutral-500/10",
                                short: label,
                              };
                              return (
                                <span
                                  key={label}
                                  className={cn(
                                    "px-1.5 py-0.5 rounded font-mono",
                                    style.text,
                                    style.bg,
                                  )}
                                >
                                  {style.short} {count}
                                </span>
                              );
                            })}
                        </div>
                      );
                    })()}
                  <div className="flex flex-1 min-h-0">
                  <Virtuoso
                    ref={virtuosoRef}
                    style={{ flex: 1, minHeight: 0 }}
                    data={flatTimeline}
                    rangeChanged={(range: ListRange) => {
                      visibleRangeRef.current = {
                        startIndex: range.startIndex,
                      };
                      setVisibleStart(range.startIndex);
                    }}
                    endReached={() => {
                      if (hasMore && !loadingMore) loadMoreMessages();
                    }}
                    overscan={300}
                    itemContent={(_index: number, item: FlatItem) => {
                      if (item.type === "date-header") {
                        return (
                          <div className="flex items-center gap-3 my-3">
                            <div className="flex-1 h-px bg-neutral-800/50" />
                            <span className="text-[10px] text-neutral-600">
                              {item.date}
                            </span>
                            <div className="flex-1 h-px bg-neutral-800/50" />
                          </div>
                        );
                      }
                      if (item.type === "tool-pair") {
                        return (
                          <ToolPairBubble
                            call={item.call}
                            result={item.result}
                            labels={
                              showLabels
                                ? labelsMap[String(item.call.id)]
                                : undefined
                            }
                          />
                        );
                      }
                      if (item.type === "message") {
                        return (
                          <MessageBubble
                            msg={item.data}
                            jsonlPath={jsonlPath}
                            labels={
                              showLabels
                                ? labelsMap[String(item.data.id)]
                                : undefined
                            }
                          />
                        );
                      }
                      if (item.type === "turn-header") {
                        return <TurnHeaderBubble turn={item.turn} />;
                      }
                      return <EventBubble event={item.data} />;
                    }}
                    components={{
                      Footer: () =>
                        hasMore ? (
                          <div className="flex justify-center py-4">
                            <span className="text-xs text-neutral-600">
                              {loadingMore ? "加载中..." : ""}
                            </span>
                          </div>
                        ) : null,
                    }}
                  />
                  {/* User message minimap sidebar */}
                  {userIndex.length > 0 && (
                  <UserMessageMinimap
                    userIndex={userIndex}
                    flatTimeline={flatTimeline}
                    visibleStartIndex={visibleStart}
                    onJump={(index) => {
                      virtuosoRef.current?.scrollToIndex({ index, align: "start" });
                    }}
                    onLoadAround={async (messageId) => {
                      // Set pending scroll target, then reload messages around the target
                      pendingScrollToMsgId.current = messageId;
                      const sinceId = Math.max(0, messageId - 250);
                      try {
                        const res = await fetch(
                          `/api/conversations?sessionId=${encodeURIComponent(selectedId)}&sinceId=${sinceId}&tail=500`,
                        );
                        if (res.ok) {
                          const data = await res.json();
                          const msgs: ConversationMessage[] = data.messages || [];
                          if (msgs.length > 0) {
                            setMessages(msgs);
                            setHasMore(msgs.length >= 500);
                          }
                        }
                      } catch { /* silent */ }
                    }}
                  />
                  )}
                  </div>
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
