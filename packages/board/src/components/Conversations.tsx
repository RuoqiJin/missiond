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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectSeparator,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { FolderOpen } from "lucide-react";

interface Conversation {
  id: string;
  uiId?: string;
  displayId?: string;
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
  updatedAt?: string | null;
  endedAt: string | null;
  status: string;
  conversationType: string;
  chatType: string | null;
  topicId?: string | null;
  topicLabel?: string | null;
  providerTitle?: string | null;
  displayTitle?: string | null;
  llmSummary: string | null;
  labels?: [string, string][];
}

interface ConversationMessage {
  id: number;
  sessionId: string;
  role: string;
  rawRole?: string | null;
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

type ConversationViewMode =
  | "conversations"
  | "workers"
  | "gemini"
  | "jarvis"
  | "codex";

interface ConversationsProps {
  fixedViewMode?: ConversationViewMode;
  storageScope?: string;
  hideViewTabs?: boolean;
}

interface CodexToolCall {
  callId: string;
  messageId: number | null;
  toolName: string;
  namespace?: string | null;
  displayTitle?: string | null;
  inputSummary: string | null;
  rawInput: string | null;
  outputSummary: string | null;
  rawOutput: string | null;
  outputImages?: { lineNo: number; index: number; mediaType: string }[];
  status: string;
  durationMs: number | null;
  timestamp: string;
  outputTimestamp?: string | null;
  source?: string | null;
  rawCallJson?: string | null;
  rawOutputJson?: string | null;
  lineNo?: number | null;
  outputLineNo?: number | null;
}

interface CodexToolCallGroup {
  id: string;
  calls: CodexToolCall[];
  timestamp: string;
  title: string;
  subtitle: string | null;
  previewLines: string[];
  status: "success" | "error" | "mixed" | "pending";
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

const CODEX_REQUEST_MARKER = "## My request for Codex:";
const CODEX_BOTTOM_THRESHOLD_PX = 96;

function extractUserDisplayContent(content: string | null | undefined): string {
  const text = (content || "").trim();
  const markerIndex = text.indexOf(CODEX_REQUEST_MARKER);
  if (markerIndex < 0) return text;

  const requestText = text.slice(markerIndex + CODEX_REQUEST_MARKER.length).trim();
  return requestText || text;
}

function makeUserPreview(content: string | null | undefined, maxLength = 120): string {
  const preview = extractUserDisplayContent(content).replace(/\s+/g, " ").trim();
  return preview.length > maxLength ? preview.slice(0, maxLength) : preview;
}

function compactConversationText(value: string | null | undefined, maxLength = 140): string | null {
  const compact = (value || "").replace(/\s+/g, " ").trim();
  if (!compact) return null;
  return compact.length > maxLength ? `${compact.slice(0, maxLength)}...` : compact;
}

function conversationUniqueKey(conv: Conversation): string {
  if (conv.uiId) return conv.uiId;
  if (conv.jsonlPath) return `${conv.id}:${conv.jsonlPath}`;
  return conv.id;
}

function conversationDomIdFromKey(key: string): string {
  return `conv-${key.replace(/[^a-zA-Z0-9_-]/g, "_")}`;
}

function conversationDomId(conv: Conversation): string {
  return conversationDomIdFromKey(conversationUniqueKey(conv));
}

function conversationShortId(conv: Conversation): string {
  return conv.displayId || conv.id.slice(0, 12);
}

function conversationTitle(conv: Conversation): string | null {
  return (
    compactConversationText(conv.displayTitle) ||
    compactConversationText(conv.providerTitle) ||
    compactConversationText(conv.topicLabel) ||
    compactConversationText(conv.llmSummary) ||
    null
  );
}

function conversationProjectName(conv: Conversation): string | null {
  if (!conv.project) return null;
  return conv.project.split("/").filter(Boolean).pop() || conv.project;
}

function conversationDetailTitle(conv: Conversation): string {
  return conversationTitle(conv) || conversationProjectName(conv) || conv.id;
}

function conversationSecondarySummary(conv: Conversation, title: string | null): string | null {
  const summary = compactConversationText(conv.llmSummary, 180);
  if (!summary || summary === title) return null;
  return summary;
}

function isCodexPtyPlaceholder(conv: Conversation): boolean {
  return (
    conv.source === "codex_cli" &&
    !conv.jsonlPath &&
    conv.id.startsWith("pty-") &&
    conv.messageCount === 0
  );
}

function conversationDisplayStatus(
  conv: Conversation,
): "active" | "completed" | "compacted" | "placeholder" {
  if (isCodexPtyPlaceholder(conv)) return "placeholder";
  if (conv.status === "compacted") return "compacted";
  if (conv.status === "active") return "active";
  return "completed";
}

function conversationStatusLabel(conv: Conversation): string {
  const status = conversationDisplayStatus(conv);
  if (status === "active") return "进行中";
  if (status === "compacted") return "已压缩";
  if (status === "placeholder") return "占位";
  return "已完成";
}

function conversationStatusClass(conv: Conversation): string {
  const status = conversationDisplayStatus(conv);
  if (status === "active") return "text-emerald-300";
  if (status === "compacted") return "text-amber-300";
  if (status === "placeholder") return "text-amber-300/70";
  return "text-stone-600";
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
  worker_user: { icon: Terminal, color: "text-cyan-400", label: "工位输入" },
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
          "cursor-pointer rounded-lg border border-white/10 shadow-sm transition-all hover:border-teal-300/40",
          expanded ? "max-w-full" : "max-w-sm max-h-64 object-cover",
        )}
        onClick={() => setExpanded(!expanded)}
        loading="lazy"
      />
    </div>
  );
}

function memoryCitationCount(block: string): number {
  const match = block.match(/<citation_entries>\s*([\s\S]*?)\s*<\/citation_entries>/);
  if (!match) return 0;
  return match[1].split(/\r?\n/).map((line) => line.trim()).filter(Boolean).length;
}

function CollapsedRuntimeBlock({
  label,
  raw,
  tone,
}: {
  label: string;
  raw: string;
  tone: "memory" | "abort";
}) {
  const [expanded, setExpanded] = useState(false);
  return (
    <div
      className={cn(
        "my-2 rounded-md border px-2 py-1 text-xs",
        tone === "memory"
          ? "border-cyan-500/20 bg-cyan-500/5 text-cyan-300"
          : "border-rose-500/20 bg-rose-500/5 text-rose-300",
      )}
    >
      <button
        type="button"
        onClick={() => setExpanded((value) => !value)}
        className="flex w-full items-center gap-1 text-left font-medium"
      >
        <ChevronRight
          className={cn(
            "h-3 w-3 shrink-0 transition-transform",
            expanded && "rotate-90",
          )}
        />
        {label}
      </button>
      {expanded && (
        <pre className="mission-code-surface mt-2 max-h-56 overflow-auto whitespace-pre-wrap rounded-md p-2 font-mono text-[10px] text-stone-500">
          {raw}
        </pre>
      )}
    </div>
  );
}

function MessageTextContent({ content }: { content: string }) {
  const blocks = useMemo(() => {
    const parts: Array<
      | { type: "text"; value: string }
      | { type: "memory"; value: string; count: number }
      | { type: "abort"; value: string }
    > = [];
    const pattern = /<oai-mem-citation>[\s\S]*?<\/oai-mem-citation>|<turn_aborted>[\s\S]*?<\/turn_aborted>/g;
    let lastIndex = 0;
    let match: RegExpExecArray | null;
    while ((match = pattern.exec(content)) != null) {
      if (match.index > lastIndex) {
        parts.push({ type: "text", value: content.slice(lastIndex, match.index) });
      }
      const value = match[0];
      if (value.startsWith("<oai-mem-citation>")) {
        parts.push({ type: "memory", value, count: memoryCitationCount(value) });
      } else {
        parts.push({ type: "abort", value });
      }
      lastIndex = match.index + value.length;
    }
    if (lastIndex < content.length) {
      parts.push({ type: "text", value: content.slice(lastIndex) });
    }
    return parts;
  }, [content]);

  if (blocks.length === 1 && blocks[0]?.type === "text") return <>{content}</>;

  return (
    <>
      {blocks.map((block, index) => {
        if (block.type === "text") return <span key={index}>{block.value}</span>;
        if (block.type === "memory") {
          const count = block.count || 0;
          return (
            <CollapsedRuntimeBlock
              key={index}
              label={`${count} 条记忆引用`}
              raw={block.value}
              tone="memory"
            />
          );
        }
        return (
          <CollapsedRuntimeBlock
            key={index}
            label="用户终止"
            raw={block.value}
            tone="abort"
          />
        );
      })}
    </>
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
            return (
              <MessageTextContent key={i} content={(block.text as string) || ""} />
            );
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
  return <MessageTextContent content={msg.content} />;
}

function rawContentHasImage(rawContent: string | null | undefined): boolean {
  if (!rawContent) return false;
  try {
    const blocks = JSON.parse(rawContent);
    return Array.isArray(blocks) &&
      blocks.some((block: Record<string, unknown>) => block?.type === "image");
  } catch {
    return rawContent.includes('"type":"image"') ||
      rawContent.includes('"type": "image"');
  }
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
    <div className="mission-code-surface my-1 overflow-hidden rounded-md border">
      <button
        onClick={() => setExpanded((v) => !v)}
        className="flex w-full items-center gap-2 bg-white/[0.025] px-3 py-1.5 text-left transition-colors hover:bg-white/[0.05]"
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
        <span className="hidden truncate text-[10px] text-stone-600 sm:inline">
          {filePath}
        </span>
        <span className="ml-auto flex-shrink-0 text-[10px] text-stone-600">
          {lineCount} lines · {lang}
        </span>
      </button>
      {expanded &&
        (isMarkdown ? (
          <div className="px-3 py-2 max-h-[600px] overflow-auto">
            <MarkdownContent content={cleaned} />
          </div>
        ) : (
          <pre className="max-h-[600px] overflow-auto px-3 py-2 font-mono text-xs text-stone-300">
            {cleaned.split("\n").map((line, i) => (
              <div key={i} className="flex">
                <span className="w-10 flex-shrink-0 select-none pr-3 text-right text-stone-700">
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
    <div className="mission-code-surface my-1 overflow-hidden rounded-md border">
      <button
        onClick={() => setExpanded((v) => !v)}
        className="flex w-full items-center gap-2 bg-white/[0.025] px-3 py-1.5 text-left transition-colors hover:bg-white/[0.05]"
      >
        <ChevronRight
          className={cn(
            "w-3 h-3 text-neutral-500 transition-transform",
            expanded && "rotate-90",
          )}
        />
        <span className="text-xs font-mono text-amber-400">{fileName}</span>
        {isSuccess && (
          <span className="text-[10px] text-emerald-300">updated</span>
        )}
        <span className="ml-auto hidden truncate text-[10px] text-stone-600 sm:inline">
          {filePath}
        </span>
      </button>
      {expanded && (
        <div className="whitespace-pre-wrap px-3 py-2 text-xs text-stone-400">
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
    <div className="mission-code-surface my-1 overflow-hidden rounded-md border">
      <button
        onClick={() => setExpanded((v) => !v)}
        className="flex w-full items-center gap-2 bg-white/[0.025] px-3 py-1.5 text-left transition-colors hover:bg-white/[0.05]"
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
        <span className="ml-auto flex-shrink-0 text-[10px] text-stone-600">
          {lines.length} lines
        </span>
      </button>
      {expanded && (
        <div>
          <div className="break-all border-b border-white/[0.06] px-3 py-1 font-mono text-[10px] text-stone-600">
            $ {command}
          </div>
          <pre className="max-h-[400px] overflow-auto whitespace-pre-wrap px-3 py-2 font-mono text-xs text-stone-300">
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
      <div className="whitespace-pre-wrap break-words font-mono text-xs text-stone-500">
        {call.content}
      </div>
      {isLarge && !expanded ? (
        <button
          onClick={() => setExpanded(true)}
          className="border-l-2 border-teal-400/25 pl-2 text-[11px] text-teal-300 hover:text-teal-200"
        >
          ▶ 展开结果 ({result.content.length.toLocaleString()} chars)
        </button>
      ) : (
        <div className="max-h-[300px] overflow-auto whitespace-pre-wrap break-words border-l-2 border-white/[0.08] pl-2 text-xs text-stone-400">
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
    <div className="mission-tool-row px-3 py-2">
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
          工具调用 (msg {call.id})
        </span>
        <span className="rounded-md border border-amber-400/15 bg-amber-400/10 px-1.5 py-0.5 font-mono text-xs text-amber-200">
          {toolName}
        </span>
        {labels && labels.length > 0 && <LabelBadges labels={labels} />}
        {!expanded && (
          <span className="ml-1 max-w-[400px] truncate font-mono text-xs text-stone-600">
            {preview}
          </span>
        )}
        <span className="ml-auto font-mono text-[10px] text-teal-300/45">
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

function formatRawPayload(value: string | null): string {
  if (!value) return "";
  try {
    return JSON.stringify(JSON.parse(value), null, 2);
  } catch {
    return value;
  }
}

function parseRawPayload(value: string | null): Record<string, unknown> | null {
  if (!value) return null;
  try {
    const parsed = JSON.parse(value);
    return parsed && typeof parsed === "object" && !Array.isArray(parsed)
      ? (parsed as Record<string, unknown>)
      : null;
  } catch {
    return null;
  }
}

function compactToolText(value: string | null | undefined, maxLength = 90): string | null {
  const compact = (value || "").replace(/\s+/g, " ").trim();
  if (!compact) return null;
  return compact.length > maxLength ? `${compact.slice(0, maxLength)}...` : compact;
}

function basename(value: string): string {
  return value.split(/[\\/]/).filter(Boolean).pop() || value;
}

function stripShellQuotes(value: string): string {
  return value.replace(/^['"]|['"]$/g, "");
}

function shellTokens(command: string): string[] {
  return (command.match(/"[^"]*"|'[^']*'|\S+/g) || []).map(stripShellQuotes);
}

function commandFromToolCall(call: CodexToolCall): string | null {
  const args = parseRawPayload(call.rawInput);
  const cmd = args?.cmd ?? args?.command;
  return typeof cmd === "string" ? cmd : null;
}

function extractReadTarget(command: string): string | null {
  const firstCommand = command.split(/[|;]/)[0]?.trim() || "";
  const tokens = shellTokens(firstCommand);
  const binary = basename(tokens[0] || "");
  if (!["nl", "cat", "sed"].includes(binary)) return null;

  for (let i = tokens.length - 1; i >= 1; i--) {
    const token = tokens[i];
    if (!token || token.startsWith("-") || token === "{}") continue;
    if (binary === "sed" && /^[$,./0-9]+[a-z]*$/i.test(token)) continue;
    return token;
  }
  return null;
}

function commandVerb(command: string): string {
  return command.trim().split(/\s+/)[0] || "command";
}

function describeCodexToolCall(call: CodexToolCall): {
  kind: "read" | "search" | "edit" | "plan" | "browser" | "command" | "tool";
  line: string;
  target: string | null;
} {
  const toolName = call.toolName;
  const command = commandFromToolCall(call);
  const inputSummary = compactToolText(call.inputSummary, 110);

  if (toolName === "exec_command" && command) {
    const readTarget = extractReadTarget(command);
    if (readTarget) {
      return { kind: "read", line: `Read ${basename(readTarget)}`, target: readTarget };
    }
    if (/^\s*(rg|grep)\b/.test(command)) {
      return { kind: "search", line: compactToolText(`Searched ${command}`, 120) || "Searched repository", target: null };
    }
    if (/^\s*(find|ls)\b/.test(command)) {
      return { kind: "search", line: compactToolText(`Listed ${command}`, 120) || "Listed files", target: null };
    }
    if (/^\s*git\s+(diff|show|status|log)\b/.test(command)) {
      return { kind: "search", line: compactToolText(`Inspected ${command}`, 120) || "Inspected git state", target: null };
    }
    return {
      kind: "command",
      line: compactToolText(`Ran ${command}`, 120) || `Ran ${commandVerb(command)}`,
      target: null,
    };
  }

  if (toolName === "apply_patch") {
    return { kind: "edit", line: inputSummary || "Edited files", target: null };
  }
  if (toolName === "update_plan") {
    return { kind: "plan", line: "Updated plan", target: null };
  }
  if (toolName.includes("browser") || call.namespace === "browser") {
    return { kind: "browser", line: inputSummary || `Used ${toolName}`, target: null };
  }

  return {
    kind: "tool",
    line: inputSummary || call.displayTitle || toolName || "Used tool",
    target: null,
  };
}

function codexToolGroupStatus(calls: CodexToolCall[]): CodexToolCallGroup["status"] {
  const statuses = new Set(calls.map((call) => call.status));
  if (statuses.has("error")) return statuses.size === 1 ? "error" : "mixed";
  if (statuses.has("pending")) return "pending";
  if (statuses.size === 1 && statuses.has("success")) return "success";
  return statuses.size > 1 ? "mixed" : "pending";
}

function buildCodexToolCallGroup(calls: CodexToolCall[]): CodexToolCallGroup {
  const descriptions = calls.map(describeCodexToolCall);
  const counts = descriptions.reduce(
    (acc, item) => {
      acc[item.kind] = (acc[item.kind] || 0) + 1;
      return acc;
    },
    {} as Record<string, number>,
  );
  const readCount = counts.read || 0;
  const searchCount = counts.search || 0;
  const editCount = counts.edit || 0;
  const commandCount = counts.command || 0;
  const planCount = counts.plan || 0;

  let title = `已使用 ${calls.length} 个工具`;
  if (readCount >= Math.max(2, calls.length - 1)) {
    title = `已探索 ${readCount} 个文件`;
  } else if (searchCount >= Math.max(2, calls.length - 1)) {
    title = `已搜索 ${searchCount} 次`;
  } else if (editCount >= Math.max(1, calls.length - 1)) {
    title = `已编辑 ${editCount} 个文件`;
  } else if (commandCount >= Math.max(2, calls.length - 1)) {
    title = `已运行 ${commandCount} 条命令`;
  }

  const extras = [
    planCount > 0 ? `${planCount} 次计划更新` : null,
    editCount > 0 && !title.includes("编辑") ? `${editCount} 次编辑` : null,
    searchCount > 0 && !title.includes("搜索") ? `${searchCount} 次搜索` : null,
    commandCount > 0 && !title.includes("运行") ? `${commandCount} 条命令` : null,
  ].filter((item): item is string => Boolean(item));

  const previewLines = descriptions.map((item) => item.line);
  return {
    id: calls.map((call) => call.callId).join(":"),
    calls,
    timestamp: calls[0]?.timestamp || new Date(0).toISOString(),
    title,
    subtitle: extras.length > 0 ? extras.join(" · ") : null,
    previewLines,
    status: codexToolGroupStatus(calls),
  };
}

function CodexToolCallPayloadDetails({
  call,
  jsonlPath,
}: {
  call: CodexToolCall;
  jsonlPath?: string | null;
}) {
  const rawInput = formatRawPayload(call.rawInput);
  const rawOutput = formatRawPayload(call.rawOutput);
  const outputImages = call.outputImages || [];

  return (
    <div className="space-y-2">
      <div className="flex flex-wrap items-center gap-2 text-[10px] text-stone-600">
        <span className="font-mono">{call.callId}</span>
        {call.messageId != null && <span>message_id:{call.messageId}</span>}
        {call.lineNo != null && <span>call line:{call.lineNo}</span>}
        {call.outputLineNo != null && <span>output line:{call.outputLineNo}</span>}
        {call.outputTimestamp && <span>output:{formatTime(call.outputTimestamp)}</span>}
        {call.source && <span>{call.source}</span>}
      </div>
      {call.inputSummary && (
        <div className="mission-code-surface rounded border px-2 py-1 font-mono text-xs text-stone-400">
          {call.inputSummary}
        </div>
      )}
      {outputImages.length > 0 && jsonlPath && (
        <div className="flex flex-wrap gap-2">
          {outputImages.map((image) => {
            const src = `/api/conversation-image?path=${encodeURIComponent(jsonlPath)}&toolLine=${image.lineNo}&index=${image.index}`;
            return (
              // eslint-disable-next-line @next/next/no-img-element
              <img
                key={`${image.lineNo}:${image.index}`}
                src={src}
                alt={call.displayTitle || call.toolName}
                className="max-h-72 max-w-sm rounded-lg border border-white/10 object-contain shadow-sm"
                loading="lazy"
              />
            );
          })}
        </div>
      )}
      {rawInput && (
        <div className="mission-code-surface overflow-hidden rounded border">
          <div className="border-b border-white/[0.06] bg-white/[0.025] px-2 py-1 font-mono text-[10px] text-stone-500">
            raw input
          </div>
          <pre className="max-h-[360px] overflow-auto whitespace-pre-wrap px-2 py-2 text-xs text-stone-300">
            {rawInput}
          </pre>
        </div>
      )}
      {rawOutput && (
        <div className="mission-code-surface overflow-hidden rounded border">
          <div className="border-b border-white/[0.06] bg-white/[0.025] px-2 py-1 font-mono text-[10px] text-stone-500">
            raw output
          </div>
          <pre className="max-h-[420px] overflow-auto whitespace-pre-wrap px-2 py-2 text-xs text-stone-300">
            {rawOutput}
          </pre>
        </div>
      )}
    </div>
  );
}

function CodexToolCallDetailRow({
  call,
  jsonlPath,
}: {
  call: CodexToolCall;
  jsonlPath?: string | null;
}) {
  const [expanded, setExpanded] = useState(false);
  const description = describeCodexToolCall(call);
  const toolLabel = call.namespace ? `${call.namespace}.${call.toolName}` : call.toolName;
  const statusTone =
    call.status === "success"
      ? "text-emerald-300"
      : call.status === "error"
        ? "text-red-300"
        : "text-amber-300";

  return (
    <div className="border-t border-white/[0.055] first:border-t-0">
      <button
        onClick={() => setExpanded((v) => !v)}
        className="flex w-full items-center gap-2 py-1.5 text-left"
      >
        <ChevronRight
          className={cn(
            "w-3 h-3 shrink-0 text-neutral-600 transition-transform",
            expanded && "rotate-90",
          )}
        />
        <span className="min-w-0 flex-1 truncate text-xs text-stone-400">
          {description.line}
        </span>
        <span className="hidden max-w-[160px] truncate font-mono text-[10px] text-stone-700 sm:inline">
          {toolLabel}
        </span>
        <span className={cn("shrink-0 font-mono text-[10px]", statusTone)}>
          {call.status}
        </span>
      </button>
      {expanded && (
        <div className="pb-2 pl-5">
          <CodexToolCallPayloadDetails call={call} jsonlPath={jsonlPath} />
        </div>
      )}
    </div>
  );
}

function CodexToolCallGroupBubble({
  group,
  jsonlPath,
}: {
  group: CodexToolCallGroup;
  jsonlPath?: string | null;
}) {
  const [expanded, setExpanded] = useState(false);
  const statusTone =
    group.status === "success"
      ? "text-emerald-300"
      : group.status === "error"
        ? "text-red-300"
        : group.status === "mixed"
          ? "text-orange-300"
          : "text-amber-300";
  const preview = group.previewLines.slice(0, 4);

  return (
    <div className="px-3 py-2">
      <button
        onClick={() => setExpanded((v) => !v)}
        className="flex w-full items-center gap-2 text-left"
      >
        <Terminal className="w-3.5 h-3.5 shrink-0 text-stone-500" />
        <span className="min-w-0 truncate text-sm font-semibold text-stone-200">
          {group.title}
        </span>
        {group.subtitle && (
          <span className="hidden min-w-0 truncate text-xs text-stone-600 md:inline">
            {group.subtitle}
          </span>
        )}
        <ChevronDown
          className={cn(
            "ml-1 w-3.5 h-3.5 shrink-0 text-stone-600 transition-transform",
            !expanded && "-rotate-90",
          )}
        />
        <span className={cn("ml-auto shrink-0 font-mono text-[10px]", statusTone)}>
          {group.status}
        </span>
        <span className="shrink-0 font-mono text-[10px] text-teal-300/45">
          {formatTime(group.timestamp)}
        </span>
      </button>
      {expanded ? (
        <div className="mt-2 pl-5">
          <div className="border-l border-white/[0.075] pl-3">
            {group.calls.map((call) => (
              <CodexToolCallDetailRow
                key={call.callId}
                call={call}
                jsonlPath={jsonlPath}
              />
            ))}
          </div>
        </div>
      ) : (
        preview.length > 0 && (
          <div className="mt-1 space-y-0.5 pl-6">
            {preview.map((line, index) => (
              <div key={`${line}:${index}`} className="truncate text-xs text-stone-600">
                {line}
              </div>
            ))}
            {group.previewLines.length > preview.length && (
              <div className="text-xs text-stone-700">
                +{group.previewLines.length - preview.length} 条
              </div>
            )}
          </div>
        )
      )}
    </div>
  );
}

function CodexToolCallBubble({
  call,
  jsonlPath,
}: {
  call: CodexToolCall;
  jsonlPath?: string | null;
}) {
  const [expanded, setExpanded] = useState(false);
  const outputImages = call.outputImages || [];
  const rawInput = formatRawPayload(call.rawInput);
  const displayTitle = call.displayTitle || "工具调用";
  const toolLabel = call.namespace ? `${call.namespace}.${call.toolName}` : call.toolName;
  const statusTone =
    call.status === "success"
      ? "text-emerald-300 bg-emerald-500/10 border-emerald-500/20"
      : call.status === "error"
        ? "text-red-300 bg-red-500/10 border-red-500/20"
        : "text-amber-300 bg-amber-500/10 border-amber-500/20";
  const preview =
    (call.displayTitle && call.inputSummary !== call.displayTitle ? call.inputSummary : null) ||
    (rawInput ? rawInput.slice(0, 160).replace(/\n/g, " ") : "") ||
    call.outputSummary ||
    "";

  return (
    <div className="mission-tool-row px-3 py-2">
      <button
        onClick={() => setExpanded((v) => !v)}
        className="flex w-full items-center gap-2 text-left"
      >
        <ChevronRight
          className={cn(
            "w-3 h-3 shrink-0 text-neutral-600 transition-transform",
            expanded && "rotate-90",
          )}
        />
        <Wrench className="w-3.5 h-3.5 shrink-0 text-amber-400" />
        <span className="min-w-0 truncate text-sm font-semibold text-amber-300">
          {displayTitle}
        </span>
        <span className="max-w-[180px] truncate rounded-md border border-amber-400/15 bg-amber-400/10 px-1.5 py-0.5 font-mono text-xs text-amber-200">
          {toolLabel}
        </span>
        <span className="hidden max-w-[170px] truncate font-mono text-[10px] text-stone-600 sm:inline">
          {call.callId}
        </span>
        <span className={cn("shrink-0 rounded border px-1.5 py-0.5 text-[10px] font-mono", statusTone)}>
          {call.status}
        </span>
        {call.durationMs != null && (
          <span className="shrink-0 font-mono text-[10px] text-stone-600">
            {call.durationMs}ms
          </span>
        )}
        {outputImages.length > 0 && (
          <span className="shrink-0 rounded border border-teal-400/20 bg-teal-400/10 px-1.5 py-0.5 font-mono text-[10px] text-teal-200">
            {outputImages.length} screenshot{outputImages.length > 1 ? "s" : ""}
          </span>
        )}
        {!expanded && preview && (
          <span className="min-w-0 flex-1 truncate font-mono text-xs text-stone-600">
            {preview}
          </span>
        )}
        <span className="ml-auto shrink-0 font-mono text-[10px] text-teal-300/45">
          {formatTime(call.timestamp)}
        </span>
      </button>

      {expanded && (
        <div className="mt-2 space-y-2 pl-5">
          <CodexToolCallPayloadDetails call={call} jsonlPath={jsonlPath} />
        </div>
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
    <div className="mission-minimap w-36 flex-shrink-0 overflow-y-auto border-l">
      <div className="mission-minimap-header sticky top-0 z-10 flex items-center gap-1.5 border-b px-2 py-1.5 text-[9px] text-stone-500">
        <User className="h-3 w-3 text-teal-300/70" />
        {allMarkers.length} 条用户消息
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
                "w-full border-l-2 px-2 py-1 text-left transition-colors",
                isActive
                  ? "mission-minimap-active"
                  : "border-transparent hover:bg-white/[0.045]",
                !isLoaded && "opacity-50",
              )}
              title={isLoaded ? m.preview : `${m.preview}\n(点击加载此区域)`}
            >
              <div className="flex items-center gap-1">
                <div className={cn(
                  "w-1.5 h-1.5 rounded-full flex-shrink-0",
                  isActive ? "bg-teal-300" : "bg-teal-300/45",
                )} />
                <span className={cn("text-[9px]", isActive ? "text-teal-100" : "text-stone-600")}>{m.time}</span>
              </div>
              <p className={cn(
                "text-[10px] truncate pl-2.5",
                isActive ? "text-teal-50/75" : "text-stone-600",
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
  "system",
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
    rawContentHasImage(msg.rawContent) ||
    msg.content.includes("[图片]");

  // Extract a short preview for collapsed state
  const preview = isCollapsible
    ? msg.content.slice(0, 80).replace(/\n/g, " ") +
      (msg.content.length > 80 ? "…" : "")
    : "";

  return (
    <div className="mission-message-row py-3.5">
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
          {isSlot ? msg.roleDisplay : msg.roleDisplay || config.label} (msg {msg.id})
        </span>
        {msg.toolName && (
          <span className="rounded-md border border-amber-400/15 bg-amber-400/10 px-1.5 py-0.5 font-mono text-xs text-amber-200">
            {msg.toolName}
          </span>
        )}
        {labels && labels.length > 0 && <LabelBadges labels={labels} />}
        {!expanded && (
          <span className="ml-1 max-w-[400px] truncate text-xs text-stone-500">
            {preview}
          </span>
        )}
        {!expanded && (
          <span className="ml-auto flex-shrink-0 text-[10px] text-stone-600">
            {msg.content.length.toLocaleString()} chars
          </span>
        )}
      </div>

      {expanded && (
        <>
          {/* Timestamp line */}
          <div className="mb-2 mt-1.5 font-mono text-xs text-teal-300/[0.68]">
            {msg.timestamp}
            {msg.model && (
              <span className="ml-3 text-stone-500">{msg.model}</span>
            )}
            {msg.seq != null && (
              <span className="ml-3 text-stone-600">seq:{msg.seq}</span>
            )}
          </div>

          {/* Content */}
          <div
            className={cn(
              "text-sm leading-relaxed whitespace-pre-wrap break-words",
              msg.role === "user" ? "text-stone-100" : "text-stone-400",
              msg.role === "thinking" && "text-purple-300/70",
              msg.role === "tool_result" && "font-mono text-xs",
            )}
          >
            {hasImages ? (
              <MessageContent msg={msg} jsonlPath={jsonlPath} />
            ) : msg.content.length > 2000 && !showFull ? (
              <>
                <div>
                  <MessageTextContent content={msg.content.slice(0, 2000)} />
                </div>
                <button
                  onClick={() => setShowFull(true)}
                  className="text-[11px] text-cyan-500 hover:text-cyan-400 mt-1"
                >
                  ▼ 展开全部 ({msg.content.length.toLocaleString()} chars)
                </button>
              </>
            ) : (
              <MessageTextContent content={msg.content} />
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
    <div className="flex items-center gap-2 py-0.5 opacity-55 transition-opacity hover:opacity-85">
      <Icon className={cn("w-3 h-3 flex-shrink-0", color)} />
      <span className={cn("text-[10px] font-mono", color)}>{label}</span>
      {event.content && (
        <span className="truncate text-[10px] text-stone-600">
          {event.content}
        </span>
      )}
      <span className="ml-auto flex-shrink-0 text-[10px] text-stone-700">
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
    <div className="my-2 flex items-stretch gap-0">
      {/* Left accent bar */}
      <div className="w-1 flex-shrink-0 rounded-l bg-teal-400/55" />
      <div className="flex-1 rounded-r border border-l-0 border-teal-400/[0.18] bg-teal-400/[0.045] px-3 py-2">
        {/* Top row: turn index + user content preview */}
        <div className="flex items-center gap-2">
          <span className="flex-shrink-0 text-[11px] font-bold text-teal-300">
            Turn {turn.turnIdx}
          </span>
          {turn.topic && (
            <span className="max-w-[300px] truncate text-[11px] text-teal-200/70">
              {turn.topic}
            </span>
          )}
          <span className="ml-auto flex-shrink-0 text-[10px] text-stone-600">
            {turn.messageCount} msgs
            {turn.toolCallCount > 0 && ` · ${turn.toolCallCount} tools`}
            {duration != null && ` · ${duration >= 60 ? `${(duration / 60).toFixed(1)}m` : `${duration.toFixed(0)}s`}`}
          </span>
        </div>
        {/* User content preview */}
        {turn.userContent && (
          <p className="mt-1 max-w-full truncate text-[11px] text-stone-400">
            用户：{makeUserPreview(turn.userContent)}
          </p>
        )}
        {/* Bottom row: tool badges + flags */}
        {(tools.length > 0 || turn.hasCodeChange || turn.hasMcpCall) && (
          <div className="flex items-center gap-1.5 mt-1.5 flex-wrap">
            {tools.map((t, i) => (
              <span key={i} className="rounded bg-white/[0.045] px-1.5 py-0.5 font-mono text-[9px] text-stone-500">
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
  const isPlaceholder = isCodexPtyPlaceholder(conv);
  const isJsonlFallback =
    conv.source === "codex_cli" && Boolean(conv.jsonlPath) && conv.chatType === "jsonl_fallback";
  const title = conversationTitle(conv);
  const summary = conversationSecondarySummary(conv, title);
  const projectName = conversationProjectName(conv);
  const shortId = conversationShortId(conv);
  return (
    <div className={cn(isSubagent && "ml-4 border-l border-white/[0.07] pl-1.5")}>
      <button
        onClick={onClick}
        title={`${conversationDetailTitle(conv)}\nID: ${conv.id}${conv.jsonlPath ? `\nJSONL: ${conv.jsonlPath}` : ""}`}
        className={cn(
          "mission-conv-list-item w-full p-3 text-left transition-colors",
          active && "mission-conv-list-item-active",
          starred && !active && "mission-conv-list-item-starred",
          isSubagent && "py-2",
        )}
      >
        <div className="flex items-center justify-between mb-1">
          <div className="flex items-center gap-2 min-w-0">
            {onToggleStar && (
              <span
                onClick={(e) => { e.stopPropagation(); onToggleStar(); }}
                className={cn("cursor-pointer flex-shrink-0 text-[12px]", starred ? "text-amber-300" : "text-stone-700 hover:text-stone-500")}
                title={starred ? "取消标星" : "标星"}
              >
                {starred ? "★" : "☆"}
              </span>
            )}
            {isSubagent && (
              <GitBranch className="w-3 h-3 flex-shrink-0 text-stone-600" />
            )}
            {conv.slotId && (
              <span className="truncate font-mono text-[10px] text-stone-600">
                {conv.slotId}
              </span>
            )}
            {!conv.slotId && projectName && (
              <span className="truncate font-mono text-[10px] text-stone-600">
                {projectName}
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
                className="flex items-center gap-0.5 rounded px-1 py-0.5 text-[10px] text-stone-500 transition-colors hover:bg-white/[0.05] hover:text-stone-200"
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
                "border-white/10 text-[10px]",
                conversationStatusClass(conv),
              )}
            >
              {conversationStatusLabel(conv)}
            </Badge>
          </div>
        </div>

        {title && (
          <p className="mt-1 line-clamp-2 text-[12px] font-medium leading-relaxed text-stone-300">
            {title}
          </p>
        )}

        <div className="mt-1 flex min-w-0 items-center gap-2 text-[10px] text-stone-600">
          <span className="font-mono text-stone-500">ID {shortId}</span>
          {conv.providerTitle && conv.source === "codex_cli" && (
            <span className="rounded bg-emerald-400/[0.08] px-1 text-[9px] text-emerald-300/60">
              Codex 命名
            </span>
          )}
        </div>

        {summary && (
          <p className="mt-1 line-clamp-2 text-[11px] leading-relaxed text-stone-500">
            {summary}
          </p>
        )}

        <div className="mt-1 flex items-center justify-between text-[11px] text-stone-500">
          <div className="flex items-center gap-2 min-w-0">
            {conv.messageCount > 0 && <span>{conv.messageCount} 条消息</span>}
            {conv.taskId && (
              <span
                className="max-w-[80px] truncate font-mono text-sky-300/60"
                title={conv.taskId}
              >
                {conv.taskId.slice(0, 8)}
              </span>
            )}
            {conv.slotId && (
              <span className="font-mono text-teal-300/60">{conv.slotId}</span>
            )}
            {conv.source === "pty_jsonl" && (
              <span className="text-[9px] text-violet-300/50">PTY</span>
            )}
            {isPlaceholder && (
              <span className="text-[9px] text-amber-300/60">旧 PTY 占位</span>
            )}
            {isJsonlFallback && (
              <span className="text-[9px] text-sky-300/60">JSONL fallback</span>
            )}
            {conv.model && (
              <span className="max-w-[100px] truncate font-mono text-stone-600">
                {conv.model}
              </span>
            )}
          </div>
          <div className="flex items-center gap-2 flex-shrink-0">
            {conv.endedAt && (
              <span className="text-[10px] text-stone-600" title="持续时间">
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
            <span>{timeAgo(conv.updatedAt || conv.startedAt)}</span>
          </div>
        </div>

        {conv.gitBranch && (
          <div className="mt-1 truncate font-mono text-[10px] text-stone-600">
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
        "mission-conv-list-item w-full px-3 py-2 text-left transition-colors",
        active && "mission-conv-list-item-active",
        starred && !active && "mission-conv-list-item-starred",
      )}
    >
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2 min-w-0">
          {onToggleStar && (
            <span
              onClick={(e) => { e.stopPropagation(); onToggleStar(); }}
              className={cn("cursor-pointer flex-shrink-0 text-[12px]", starred ? "text-amber-300" : "text-stone-700 hover:text-stone-500")}
              title={starred ? "取消标星" : "标星"}
            >
              {starred ? "★" : "☆"}
            </span>
          )}
          <Sparkles className="w-3 h-3 text-indigo-400 flex-shrink-0" />
          <span className="text-[11px] font-mono text-indigo-300/80 truncate max-w-[120px]">
            {label}
          </span>
          <span className="font-mono text-[10px] text-stone-600">
            {conv.model || "gemini"}
          </span>
          <span className="rounded bg-white/[0.045] px-1 text-[9px] text-stone-500">
            {sourceTag}
          </span>
        </div>
        <div className="flex items-center gap-2 flex-shrink-0">
          {conv.messageCount > 0 && (
            <span className="text-[10px] text-stone-600">
              {conv.messageCount} 条
            </span>
          )}
          <span
            className={cn(
              "text-[10px]",
              conv.status === "active"
                ? "text-emerald-300/70"
                : "text-stone-600",
            )}
          >
            {conv.status === "active" ? "进行中" : "已完成"}
          </span>
          <span className="text-[10px] text-stone-600">
            {timeAgo(conv.startedAt)}
          </span>
        </div>
      </div>
    </button>
  );
}

export function Conversations({
  fixedViewMode,
  storageScope = "conv",
  hideViewTabs = false,
}: ConversationsProps = {}) {
  const selectedStorageKey = `${storageScope}:selectedId`;
  const selectedKeyStorageKey = `${storageScope}:selectedKey`;
  const selectedJsonlPathStorageKey = `${storageScope}:selectedJsonlPath`;
  const viewModeStorageKey = `${storageScope}:viewMode`;
  const listScrollStorageKey = `${storageScope}:listScroll`;
  const msgScrollStorageKey = `${storageScope}:msgScrollIdx`;
  const sidebarCollapsedStorageKey = `${storageScope}:sidebarCollapsed`;
  const [conversations, setConversations] = useState<Conversation[]>([]);
  const [messages, setMessages] = useState<ConversationMessage[]>([]);
  const [events, setEvents] = useState<ConversationEvent[]>([]);
  const [codexToolCalls, setCodexToolCalls] = useState<CodexToolCall[]>([]);
  const [codexToolCallSource, setCodexToolCallSource] = useState<string | null>(null);
  const [codexToolCallError, setCodexToolCallError] = useState<string | null>(null);
  const [labelsMap, setLabelsMap] = useState<
    Record<string, [string, string][]>
  >({});
  const [showLabels, setShowLabels] = useState(false);
  const [selectedId, setSelectedId] = useState<string | null>(() =>
    sessionStorage.getItem(selectedStorageKey),
  );
  const [selectedKey, setSelectedKey] = useState<string | null>(() =>
    sessionStorage.getItem(selectedKeyStorageKey),
  );
  const [selectedJsonlPath, setSelectedJsonlPath] = useState<string | null>(() =>
    sessionStorage.getItem(selectedJsonlPathStorageKey),
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
  const [projectFilter, setProjectFilter] = useState<string>("all");
  const [projects, setProjects] = useState<
    { id: string; path: string; active: boolean; conversation_count?: number }[]
  >([]);
  const [viewMode, setViewMode] = useState<ConversationViewMode>(() => {
    if (fixedViewMode) return fixedViewMode;
    const saved = sessionStorage.getItem(viewModeStorageKey);
    return saved === "conversations" ||
      saved === "workers" ||
      saved === "gemini" ||
      saved === "jarvis" ||
      saved === "codex"
      ? saved
      : "conversations";
  });
  const [showList, setShowList] = useState(true); // mobile: toggle list/detail
  const [sidebarCollapsed, setSidebarCollapsed] = useState(
    () => sessionStorage.getItem(sidebarCollapsedStorageKey) === "1",
  );
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

  useEffect(() => {
    if (fixedViewMode && viewMode !== fixedViewMode) {
      setViewMode(fixedViewMode);
    }
  }, [fixedViewMode, viewMode]);

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
    if (selectedId) sessionStorage.setItem(selectedStorageKey, selectedId);
    else sessionStorage.removeItem(selectedStorageKey);
  }, [selectedId, selectedStorageKey]);

  useEffect(() => {
    if (selectedKey) sessionStorage.setItem(selectedKeyStorageKey, selectedKey);
    else sessionStorage.removeItem(selectedKeyStorageKey);
  }, [selectedKey, selectedKeyStorageKey]);

  useEffect(() => {
    if (selectedJsonlPath) {
      sessionStorage.setItem(selectedJsonlPathStorageKey, selectedJsonlPath);
    } else {
      sessionStorage.removeItem(selectedJsonlPathStorageKey);
    }
  }, [selectedJsonlPath, selectedJsonlPathStorageKey]);

  useEffect(() => {
    if (!fixedViewMode) sessionStorage.setItem(viewModeStorageKey, viewMode);
  }, [fixedViewMode, viewMode, viewModeStorageKey]);

  useEffect(() => {
    sessionStorage.setItem(
      sidebarCollapsedStorageKey,
      sidebarCollapsed ? "1" : "0",
    );
  }, [sidebarCollapsed, sidebarCollapsedStorageKey]);

  // Fetch project list for filter dropdown
  useEffect(() => {
    fetch("/api/projects")
      .then((res) => res.ok ? res.json() : [])
      .then((data) => {
        if (Array.isArray(data)) setProjects(data);
      })
      .catch(() => {});
  }, []);

  // Track Virtuoso visible range for scroll persistence + minimap highlight
  const visibleRangeRef = useRef<{ startIndex: number; endIndex: number }>({
    startIndex: 0,
    endIndex: 0,
  });
  const [visibleStart, setVisibleStart] = useState(0);
  const pendingScrollToMsgId = useRef<number | null>(null);

  // Save scroll positions before unload
  useEffect(() => {
    const save = () => {
      if (listScrollRef.current)
        sessionStorage.setItem(
          listScrollStorageKey,
          String(listScrollRef.current.scrollTop),
        );
      sessionStorage.setItem(
        msgScrollStorageKey,
        String(visibleRangeRef.current.startIndex),
      );
    };
    window.addEventListener("beforeunload", save);
    return () => window.removeEventListener("beforeunload", save);
  }, [listScrollStorageKey, msgScrollStorageKey]);

  const isGeminiSource = useCallback((source: string) => {
    return source === "router_chat" || source === "gemini_cli";
  }, []);

  const fetchConversations = useCallback(async (options?: { silent?: boolean }) => {
    if (!options?.silent) setLoading(true);
    try {
      // Server-side source filtering per tab. No conversationType filter —
      // frontend is a faithful DB viewer, misclassified records must be visible.
      const params = new URLSearchParams();
      if (statusFilter && viewMode !== "codex") params.set("status", statusFilter);
      params.set("limit", "300");
      params.set("conversationType", "all");

      if (viewMode === "gemini") {
        params.set("conversationType", "gemini");
      } else if (viewMode === "jarvis") {
        params.set("conversationType", "jarvis");
      } else if (viewMode === "codex") {
        params.set("conversationType", "all");
        params.set("source", "codex_cli");
      } else if (viewMode === "workers") {
        params.set("conversationType", "system");
      } else {
        params.set("conversationType", "user");
      }

      if (projectFilter && projectFilter !== "all") {
        params.set("project", projectFilter);
      }

      const res = await fetch(`/api/conversations?${params}`);
      if (res.ok) {
        const data = await res.json();
        const list: Conversation[] = Array.isArray(data) ? data : [];
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
    if (!options?.silent) setLoading(false);
  }, [statusFilter, viewMode, projectFilter]);

  const PAGE_SIZE = 500;

  const fetchMessages = useCallback(
    async (
      sessionId: string,
      withLabels?: boolean,
      options?: { silent?: boolean; jsonlPath?: string | null },
    ) => {
      if (!options?.silent) setLoadingMessages(true);
      if (!options?.silent) setSearchResults(null);
      try {
        const params = new URLSearchParams({
          sessionId,
          sinceId: "0",
          tail: String(PAGE_SIZE),
        });
        if (withLabels) params.set("labels", "1");
        if (viewMode === "codex") {
          params.set("includeCodexToolCalls", "1");
          params.set("toolLimit", "100000");
          if (options?.jsonlPath) params.set("jsonlPath", options.jsonlPath);
        }
        // Load from beginning (sinceId=0) instead of tail
        const res = await fetch(`/api/conversations?${params}`);
        if (res.ok) {
          const data = await res.json();
          const codexProjectedMessages = Array.isArray(data.codexMessages)
            ? data.codexMessages
            : null;
          const msgs = codexProjectedMessages || data.messages || [];
          setMessages(msgs);
          setEvents(data.events || []);
          setCodexToolCalls(Array.isArray(data.codexToolCalls) ? data.codexToolCalls : []);
          setCodexToolCallSource(data.codexToolCallSource || null);
          setCodexToolCallError(data.codexToolCallError || null);
          const resolvedJsonlPath = data.conversation?.jsonlPath || null;
          setJsonlPath(resolvedJsonlPath);
          if (!options?.silent && viewMode === "codex") {
            setSelectedJsonlPath(resolvedJsonlPath);
          }
          setLabelsMap(data.labels || {});
          setUserIndex(
            codexProjectedMessages
              ? codexProjectedMessages
                  .filter((m: ConversationMessage) => m.role === "user")
                  .map((m: ConversationMessage) => ({
                    id: m.id,
                    time: formatTime(m.timestamp),
                    preview: makeUserPreview(m.content),
                  }))
              : data.userIndex || [],
          );
          setTurns(codexProjectedMessages ? [] : data.turns || []);
          setHasMore(codexProjectedMessages ? false : msgs.length >= PAGE_SIZE);
        }
      } catch {
        setMessages([]);
        setEvents([]);
        setCodexToolCalls([]);
        setCodexToolCallSource(null);
        setCodexToolCallError(null);
        setJsonlPath(null);
        setLabelsMap({});
        setTurns([]);
        setHasMore(false);
      }
      if (!options?.silent) setLoadingMessages(false);
    },
    [viewMode],
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
        : viewMode === "codex" ? "&conversationType=codex_chat"
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
    const savedId = sessionStorage.getItem(selectedStorageKey);
    const savedKey = sessionStorage.getItem(selectedKeyStorageKey);
    const savedJsonlPath = sessionStorage.getItem(selectedJsonlPathStorageKey);
    const savedConversation =
      (savedKey ? conversations.find((c) => conversationUniqueKey(c) === savedKey) : null) ||
      (savedId ? conversations.find((c) => c.id === savedId) : null);
    if (savedConversation) {
      const key = conversationUniqueKey(savedConversation);
      const resolvedJsonlPath = savedJsonlPath || savedConversation.jsonlPath || null;
      setSelectedId(savedConversation.id);
      setSelectedKey(key);
      setSelectedJsonlPath(resolvedJsonlPath);
      fetchMessages(savedConversation.id, showLabels, { jsonlPath: resolvedJsonlPath });
      // Restore list scroll position after DOM updates
      requestAnimationFrame(() => {
        const savedListScroll = sessionStorage.getItem(listScrollStorageKey);
        if (savedListScroll && listScrollRef.current) {
          listScrollRef.current.scrollTop = Number(savedListScroll);
        }
      });
    }
  }, [
    loading,
    conversations,
    fetchMessages,
    showLabels,
    selectedStorageKey,
    selectedKeyStorageKey,
    selectedJsonlPathStorageKey,
    listScrollStorageKey,
  ]);

  const selectConversation = useCallback(
    (conversationOrId: Conversation | string) => {
      const conversation = typeof conversationOrId === "string"
        ? conversations.find((c) => c.id === conversationOrId)
        : conversationOrId;
      const id = typeof conversationOrId === "string" ? conversationOrId : conversationOrId.id;
      const key = conversation ? conversationUniqueKey(conversation) : id;
      const nextJsonlPath = conversation?.jsonlPath || null;
      setSelectedId(id);
      setSelectedKey(key);
      setSelectedJsonlPath(nextJsonlPath);
      setJsonlPath(nextJsonlPath);
      setShowList(false);
      fetchMessages(id, showLabels, { jsonlPath: nextJsonlPath });
    },
    [conversations, fetchMessages, showLabels],
  );

  const selectedKeyResolved = useMemo(
    () => Boolean(selectedKey && conversations.some((c) => conversationUniqueKey(c) === selectedKey)),
    [conversations, selectedKey],
  );

  const selectedConv = useMemo(
    () =>
      (selectedKeyResolved && selectedKey
        ? conversations.find((c) => conversationUniqueKey(c) === selectedKey)
        : null) || conversations.find((c) => c.id === selectedId),
    [conversations, selectedId, selectedKey, selectedKeyResolved],
  );

  const isConversationActive = useCallback(
    (conv: Conversation) =>
      selectedKeyResolved && selectedKey
        ? conversationUniqueKey(conv) === selectedKey
        : conv.id === selectedId,
    [selectedId, selectedKey, selectedKeyResolved],
  );

  useEffect(() => {
    if (viewMode !== "codex" || !selectedId) return;
    const refresh = () => {
      fetchConversations({ silent: true });
      fetchMessages(selectedId, showLabels, {
        silent: true,
        jsonlPath: selectedJsonlPath,
      });
    };
    const id = window.setInterval(refresh, 2500);
    return () => window.clearInterval(id);
  }, [viewMode, selectedId, selectedJsonlPath, showLabels, fetchConversations, fetchMessages]);

  const preferredCodexConversation = useMemo(() => {
    if (viewMode !== "codex") return null;
    return (
      conversations.find((c) => conversationDisplayStatus(c) === "active" && Boolean(c.jsonlPath)) ||
      conversations.find((c) => conversationDisplayStatus(c) === "active") ||
      conversations.find((c) => Boolean(c.jsonlPath)) ||
      conversations.find((c) => !isCodexPtyPlaceholder(c)) ||
      conversations[0] ||
      null
    );
  }, [conversations, viewMode]);

  useEffect(() => {
    if (viewMode !== "codex" || conversations.length === 0) return;
    if (!preferredCodexConversation) return;
    if (selectedKeyResolved) return;
    if (selectedId && conversations.some((c) => c.id === selectedId)) return;
    setSelectedId(preferredCodexConversation.id);
    setSelectedKey(conversationUniqueKey(preferredCodexConversation));
    setSelectedJsonlPath(preferredCodexConversation.jsonlPath || null);
    fetchMessages(preferredCodexConversation.id, showLabels, {
      jsonlPath: preferredCodexConversation.jsonlPath || null,
    });
  }, [
    viewMode,
    conversations,
    selectedId,
    selectedKey,
    selectedKeyResolved,
    preferredCodexConversation,
    fetchMessages,
    showLabels,
  ]);

  // Flatten messages and events into a single timeline array for virtual scrolling
  type FlatItem =
    | { type: "date-header"; date: string }
    | { type: "message"; data: ConversationMessage }
    | { type: "event"; data: ConversationEvent }
    | { type: "codex-tool-call"; data: CodexToolCall }
    | { type: "codex-tool-call-group"; group: CodexToolCallGroup }
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
      | { type: "event"; data: ConversationEvent }
      | { type: "codex-tool-call"; data: CodexToolCall };
    const sorted: SortedItem[] = [
      ...messages.map((m) => ({ type: "message" as const, data: m })),
      ...importantEvents.map((e) => ({ type: "event" as const, data: e })),
      ...codexToolCalls.map((call) => ({ type: "codex-tool-call" as const, data: call })),
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

      if (item.type === "codex-tool-call") {
        const groupCalls = [item.data];
        let j = i + 1;
        while (j < sorted.length) {
          const next = sorted[j];
          if (next?.type !== "codex-tool-call") break;
          if (formatDate(next.data.timestamp) !== date) break;
          groupCalls.push(next.data);
          j++;
        }
        if (groupCalls.length > 1) {
          flat.push({
            type: "codex-tool-call-group",
            group: buildCodexToolCallGroup(groupCalls),
          });
          i = j;
          continue;
        }
      }
      flat.push(item);
      i++;
    }
    return flat;
  }, [messages, events, turns, codexToolCalls]);

  const latestTimelineKey = useMemo(() => {
    const item = flatTimeline[flatTimeline.length - 1];
    if (!item) return "";
    if (item.type === "message") {
      return [
        item.type,
        item.data.id,
        item.data.timestamp,
        item.data.content.length,
      ].join(":");
    }
    if (item.type === "codex-tool-call") {
      return [
        item.type,
        item.data.callId,
        item.data.status,
        item.data.timestamp,
        item.data.outputTimestamp || "",
        item.data.rawOutput?.length || 0,
      ].join(":");
    }
    if (item.type === "codex-tool-call-group") {
      const lastCall = item.group.calls[item.group.calls.length - 1];
      return [
        item.type,
        item.group.id,
        item.group.status,
        lastCall?.status || "",
        lastCall?.outputTimestamp || lastCall?.timestamp || "",
        lastCall?.rawOutput?.length || 0,
      ].join(":");
    }
    if (item.type === "event") {
      return [item.type, item.data.id, item.data.timestamp].join(":");
    }
    if (item.type === "tool-pair") {
      return [
        item.type,
        item.call.id,
        item.result.id,
        item.result.content.length,
      ].join(":");
    }
    if (item.type === "turn-header") {
      return [item.type, item.turn.turnIdx, item.turn.endedAt || ""].join(":");
    }
    return [item.type, item.date].join(":");
  }, [flatTimeline]);

  const virtuosoRef = useRef<VirtuosoHandle>(null);
  const [codexFollowLatest, setCodexFollowLatestState] = useState(true);
  const [codexAtBottom, setCodexAtBottomState] = useState(true);
  const [codexScrollerEl, setCodexScrollerEl] = useState<HTMLElement | null>(null);
  const codexFollowLatestRef = useRef(true);
  const codexAtBottomRef = useRef(true);
  const codexInitialScrollSessionRef = useRef<string | null>(null);
  const codexProgrammaticScrollUntilRef = useRef(0);

  const setCodexFollowLatest = useCallback((next: boolean) => {
    codexFollowLatestRef.current = next;
    setCodexFollowLatestState(next);
  }, []);

  const setCodexAtBottom = useCallback((next: boolean) => {
    codexAtBottomRef.current = next;
    setCodexAtBottomState(next);
  }, []);

  const measureCodexAtBottom = useCallback(
    (el: HTMLElement | null = codexScrollerEl) => {
      if (!el) return true;
      return (
        el.scrollHeight - el.scrollTop - el.clientHeight <=
        CODEX_BOTTOM_THRESHOLD_PX
      );
    },
    [codexScrollerEl],
  );

  const handleCodexScrollerRef = useCallback(
    (ref: HTMLElement | null | Window) => {
      const next =
        ref && "scrollHeight" in ref ? (ref as HTMLElement) : null;
      setCodexScrollerEl((prev) => (prev === next ? prev : next));
    },
    [],
  );

  const scrollToLatest = useCallback(() => {
    if (flatTimeline.length === 0) return;
    codexProgrammaticScrollUntilRef.current = Date.now() + 1000;
    requestAnimationFrame(() => {
      virtuosoRef.current?.scrollToIndex({
        index: flatTimeline.length - 1,
        align: "end",
      });
      requestAnimationFrame(() => {
        virtuosoRef.current?.autoscrollToBottom();
      });
    });
  }, [flatTimeline.length]);

  const handleCodexAtBottomChange = useCallback(
    (atBottom: boolean) => {
      setCodexAtBottom(atBottom);
    },
    [setCodexAtBottom],
  );

  const handleFollowLatest = useCallback(() => {
    setCodexFollowLatest(true);
    setCodexAtBottom(true);
    scrollToLatest();
  }, [scrollToLatest, setCodexAtBottom, setCodexFollowLatest]);

  const followCodexOutput = useCallback((atBottom: boolean) => {
    return codexFollowLatestRef.current && atBottom ? "auto" : false;
  }, []);

  useEffect(() => {
    if (viewMode !== "codex" || !codexScrollerEl) return;
    let frame: number | null = null;
    const updateBottomState = () => {
      if (frame != null) window.cancelAnimationFrame(frame);
      frame = window.requestAnimationFrame(() => {
        frame = null;
        const atBottom = measureCodexAtBottom(codexScrollerEl);
        setCodexAtBottom(atBottom);
        if (atBottom) {
          codexProgrammaticScrollUntilRef.current = 0;
        } else if (Date.now() > codexProgrammaticScrollUntilRef.current) {
          setCodexFollowLatest(false);
        }
      });
    };

    codexScrollerEl.addEventListener("scroll", updateBottomState, {
      passive: true,
    });
    updateBottomState();
    return () => {
      if (frame != null) window.cancelAnimationFrame(frame);
      codexScrollerEl.removeEventListener("scroll", updateBottomState);
    };
  }, [
    viewMode,
    codexScrollerEl,
    measureCodexAtBottom,
    setCodexAtBottom,
    setCodexFollowLatest,
  ]);

  // Restore message scroll position after messages finish loading (Virtuoso)
  useEffect(() => {
    if (viewMode === "codex") return;
    if (!loadingMessages && flatTimeline.length > 0) {
      const savedIdx = sessionStorage.getItem(msgScrollStorageKey);
      if (savedIdx && virtuosoRef.current) {
        requestAnimationFrame(() => {
          virtuosoRef.current?.scrollToIndex({
            index: Number(savedIdx),
            align: "start",
          });
        });
        sessionStorage.removeItem(msgScrollStorageKey);
      }
    }
  }, [loadingMessages, flatTimeline, msgScrollStorageKey, viewMode]);

  useEffect(() => {
    if (
      viewMode !== "codex" ||
      loadingMessages ||
      flatTimeline.length === 0 ||
      !selectedId
    ) {
      return;
    }
    if (codexInitialScrollSessionRef.current === selectedId) return;
    codexInitialScrollSessionRef.current = selectedId;
    setCodexFollowLatest(true);
    setCodexAtBottom(true);
    scrollToLatest();
  }, [
    viewMode,
    loadingMessages,
    selectedId,
    flatTimeline.length,
    scrollToLatest,
    setCodexAtBottom,
    setCodexFollowLatest,
  ]);

  useEffect(() => {
    if (
      viewMode !== "codex" ||
      loadingMessages ||
      flatTimeline.length === 0 ||
      !codexFollowLatestRef.current ||
      (!codexAtBottomRef.current && !measureCodexAtBottom())
    ) {
      return;
    }
    scrollToLatest();
  }, [
    viewMode,
    loadingMessages,
    latestTimelineKey,
    flatTimeline.length,
    measureCodexAtBottom,
    scrollToLatest,
  ]);

  // Handle pending scroll to a message ID (after onLoadAround reloads messages)
  useEffect(() => {
    const targetId = pendingScrollToMsgId.current;
    if (targetId == null || flatTimeline.length === 0) return;
    const idx = flatTimeline.findIndex(
      (it) => it.type === "message" && it.data.id === targetId,
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
      if (tab === "jarvis") {
        return c.conversationType === "jarvis";
      }
      if (tab === "codex") {
        return c.source === "codex_cli";
      }
      // workers: catch-all for non-user non-gemini
      return c.conversationType !== "user" && !isGeminiSource(c.source);
    },
    [isGeminiSource],
  );

  const counts = useMemo(() => {
    const filtered = conversations.filter((c) => filterByTab(c, viewMode));
    const active = filtered.filter((c) => conversationDisplayStatus(c) === "active").length;
    const completed = filtered.filter((c) => conversationDisplayStatus(c) === "completed").length;
    const compacted = filtered.filter((c) => conversationDisplayStatus(c) === "compacted").length;
    return { active, completed, compacted, total: filtered.length };
  }, [conversations, viewMode, filterByTab]);

  const statusScopedConversations = useMemo(() => {
    if (!statusFilter) return conversations;
    return conversations.filter((c) => conversationDisplayStatus(c) === statusFilter);
  }, [conversations, statusFilter]);

  // Tab counts: current tab shows exact count, others show '…' until switched
  const tabCounts = useMemo(
    () => ({
      conversations: viewMode === "conversations" ? conversations.length : null,
      jarvis: viewMode === "jarvis" ? conversations.length : null,
      workers: viewMode === "workers" ? conversations.length : null,
      gemini: viewMode === "gemini" ? conversations.length : null,
      codex: viewMode === "codex" ? conversations.length : null,
    }),
    [conversations, viewMode],
  );

  // Group: separate subagents and compacted sessions from main list.
  // Data is already tab-filtered from the API, no client-side filterByTab needed.
  const { mainList, subagentMap } = useMemo(() => {
    const map = new Map<string, Conversation[]>();
    const main: Conversation[] = [];
    for (const conv of statusScopedConversations) {
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
      const aStatus = conversationDisplayStatus(a);
      const bStatus = conversationDisplayStatus(b);
      if (aStatus === "active" && bStatus !== "active") return -1;
      if (aStatus !== "active" && bStatus === "active") return 1;
      if (aStatus === "placeholder" && bStatus !== "placeholder") return 1;
      if (aStatus !== "placeholder" && bStatus === "placeholder") return -1;
      return (
        new Date(b.updatedAt || b.startedAt).getTime() -
        new Date(a.updatedAt || a.startedAt).getTime()
      );
    });
    return { mainList: main, subagentMap: map };
  }, [statusScopedConversations]);

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
      const activeCount = sessions.filter((s) => conversationDisplayStatus(s) === "active").length;
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
    <div className="mission-conversation-shell flex min-h-0 flex-1 overflow-hidden">
      {sidebarCollapsed && (
        <div className="mission-conversation-rail flex w-10 flex-shrink-0 flex-col items-center border-r py-2">
          <button
            onClick={() => {
              setSidebarCollapsed(false);
              setShowList(true);
            }}
            className="rounded-md p-1.5 text-stone-500 transition-colors hover:bg-white/[0.05] hover:text-stone-200"
            title="展开左侧列表"
            aria-label="展开左侧列表"
          >
            <ChevronRight className="w-4 h-4" />
          </button>
          <div className="mt-2 h-px w-5 bg-white/[0.07]" />
          <MessageSquare className="mt-3 h-4 w-4 text-stone-600" />
        </div>
      )}

      {/* Left: Conversation list */}
      {!sidebarCollapsed && (
      <div
        className={cn(
          "mission-conversation-sidebar flex w-80 flex-shrink-0 flex-col border-r",
          !showList && "hidden md:flex",
        )}
      >
        {/* View mode tabs */}
        {!hideViewTabs && (
        <div className="mission-conversation-tabs flex border-b p-1">
          <button
            onClick={() => setViewMode("conversations")}
            className={cn(
              "flex flex-1 items-center justify-center gap-1.5 rounded-md py-1.5 text-xs font-medium transition-colors",
              viewMode === "conversations"
                ? "bg-white/[0.07] text-stone-100"
                : "text-stone-500 hover:bg-white/[0.04] hover:text-stone-300",
            )}
          >
            <MessageSquare className="w-3 h-3" />
            对话
          </button>
          <button
            onClick={() => setViewMode("jarvis")}
            className={cn(
              "flex flex-1 items-center justify-center gap-1.5 rounded-md py-1.5 text-xs font-medium transition-colors",
              viewMode === "jarvis"
                ? "bg-white/[0.07] text-stone-100"
                : "text-stone-500 hover:bg-white/[0.04] hover:text-stone-300",
            )}
          >
            <Zap className="w-3 h-3" />
            Jarvis
          </button>
          <button
            onClick={() => setViewMode("workers")}
            className={cn(
              "flex flex-1 items-center justify-center gap-1.5 rounded-md py-1.5 text-xs font-medium transition-colors",
              viewMode === "workers"
                ? "bg-white/[0.07] text-stone-100"
                : "text-stone-500 hover:bg-white/[0.04] hover:text-stone-300",
            )}
          >
            <Server className="w-3 h-3" />
            后台
          </button>
          <button
            onClick={() => setViewMode("gemini")}
            className={cn(
              "flex flex-1 items-center justify-center gap-1.5 rounded-md py-1.5 text-xs font-medium transition-colors",
              viewMode === "gemini"
                ? "bg-white/[0.07] text-stone-100"
                : "text-stone-500 hover:bg-white/[0.04] hover:text-stone-300",
            )}
          >
            <Sparkles className="w-3 h-3" />
            Gemini
          </button>
          <button
            onClick={() => setViewMode("codex")}
            className={cn(
              "flex flex-1 items-center justify-center gap-1.5 rounded-md py-1.5 text-xs font-medium transition-colors",
              viewMode === "codex"
                ? "bg-white/[0.07] text-stone-100"
                : "text-stone-500 hover:bg-white/[0.04] hover:text-stone-300",
            )}
          >
            <Terminal className="w-3 h-3" />
            Codex
          </button>
        </div>
        )}

        {/* Search bar */}
        <div className="mission-conversation-filterbar space-y-2 border-b p-3">
          <div className="relative">
            <Search className="absolute left-3 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-stone-500" />
            <input
              type="text"
              placeholder="搜索对话内容..."
              value={search}
              onChange={(e) => {
                setSearch(e.target.value);
                if (!e.target.value) setSearchResults(null);
              }}
              onKeyDown={(e) => e.key === "Enter" && handleSearch()}
              className="mission-control h-8 w-full pl-9 pr-3 text-xs placeholder:text-stone-500 focus:outline-none"
            />
          </div>

          {/* Project filter */}
          {projects.length > 0 && (
            <Select value={projectFilter} onValueChange={setProjectFilter}>
              <SelectTrigger className="h-8 w-full text-xs [&>svg]:h-3 [&>svg]:w-3">
                <div className="flex items-center gap-1.5 truncate">
                  <FolderOpen className="h-3 w-3 shrink-0 text-stone-500" />
                  <SelectValue placeholder="全部项目" />
                </div>
              </SelectTrigger>
              <SelectContent className="max-h-72">
                <SelectItem value="all" className="text-xs">
                  全部项目
                </SelectItem>
                <SelectSeparator className="bg-white/[0.08]" />
                {projects
                  .filter((p) => p.active)
                  .sort((a, b) => (b.conversation_count ?? 0) - (a.conversation_count ?? 0))
                  .map((p) => (
                    <SelectItem
                      key={p.id}
                      value={p.id}
                      className="text-xs"
                    >
                      {p.id}
                      {p.conversation_count != null && (
                        <span className="ml-1.5 text-stone-500">({p.conversation_count})</span>
                      )}
                    </SelectItem>
                  ))}
              </SelectContent>
            </Select>
          )}

          {/* Filters */}
          <div className="flex items-center gap-1.5">
            <button
              onClick={() => setStatusFilter(null)}
              className={cn(
                "rounded-md border px-2 py-0.5 text-[10px] transition-colors",
                !statusFilter
                  ? "border-white/[0.12] bg-white/[0.08] text-stone-100"
                  : "border-white/[0.07] text-stone-500 hover:text-stone-300",
              )}
            >
              全部 {counts.total}
            </button>
            <button
              onClick={() => setStatusFilter("active")}
              className={cn(
                "rounded-md border px-2 py-0.5 text-[10px] transition-colors",
                statusFilter === "active"
                  ? "bg-green-500/10 text-green-400 border-green-500/30"
                  : "border-white/[0.07] text-stone-500 hover:text-stone-300",
              )}
            >
              进行中 {counts.active}
            </button>
            <button
              onClick={() => setStatusFilter("completed")}
              className={cn(
                "rounded-md border px-2 py-0.5 text-[10px] transition-colors",
                statusFilter === "completed"
                  ? "border-white/[0.12] bg-white/[0.065] text-stone-300"
                  : "border-white/[0.07] text-stone-500 hover:text-stone-300",
              )}
            >
              已完成 {counts.completed}
            </button>
            <button
              onClick={() => fetchConversations()}
              className="ml-auto rounded p-1 text-stone-600 transition-colors hover:bg-white/[0.04] hover:text-stone-300"
              title="刷新"
            >
              <RefreshCw className={cn("w-3 h-3", loading && "animate-spin")} />
            </button>
            <button
              onClick={toggleCollapseAll}
              className="rounded p-1 text-stone-600 transition-colors hover:bg-white/[0.04] hover:text-stone-300"
              title={isAllCollapsed ? "展开全部" : "折叠全部"}
            >
              {isAllCollapsed ? (
                <ChevronsUpDown className="w-3 h-3" />
              ) : (
                <ChevronsDownUp className="w-3 h-3" />
              )}
            </button>
            <button
              onClick={() => {
                setSidebarCollapsed(true);
                setShowList(false);
              }}
              className="rounded p-1 text-stone-600 transition-colors hover:bg-white/[0.04] hover:text-stone-300"
              title="收起左侧列表"
              aria-label="收起左侧列表"
            >
              <ArrowLeft className="w-3 h-3" />
            </button>
          </div>
        </div>

        {/* Search results */}
        {searchResults !== null ? (
          <div className="flex-1 space-y-1 overflow-auto p-2">
            <div className="flex items-center justify-between px-1 mb-2">
              <span className="text-[11px] text-stone-500">
                搜索到 {searchResults.length} 条会话
              </span>
              <button
                onClick={() => {
                  setSearch("");
                  setSearchResults(null);
                }}
                className="text-[11px] text-stone-600 hover:text-stone-300"
              >
                清除
              </button>
            </div>
            {searchResults.map((r) => (
              <button
                key={r.sessionId || r.id}
                onClick={() => selectConversation(r.sessionId || r.id)}
                className="mission-conv-list-item w-full p-2 text-left transition-colors"
              >
                <div className="flex items-center gap-2 mb-0.5">
                  <span className="text-[10px] font-mono text-indigo-300/70 truncate max-w-[140px]">
                    {r.sessionId?.slice(0, 8) || r.id}
                  </span>
                  {r.source && (
                    <span className="rounded bg-white/[0.045] px-1 text-[9px] text-stone-500">{r.source}</span>
                  )}
                  {r.messageCount != null && (
                    <span className="text-[10px] text-stone-600">{r.messageCount} 条</span>
                  )}
                  <span className="text-[10px] text-stone-600">
                    {timeAgo(r.startedAt || r.timestamp)}
                  </span>
                </div>
                {r.summary && (
                  <p className="mb-0.5 truncate text-[11px] text-stone-400">{r.summary}</p>
                )}
                <p className="line-clamp-2 text-xs text-stone-500">
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

              const currentConv =
                (selectedKeyResolved && selectedKey
                  ? visibleList.find((c) => conversationUniqueKey(c) === selectedKey)
                  : null) || visibleList.find((c) => c.id === selectedId);

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
                    const parent = visibleList.find((c) => c.id === currentConv.parentSessionId);
                    const el = parent
                      ? document.getElementById(conversationDomId(parent))
                      : document.getElementById(conversationDomIdFromKey(currentConv.parentSessionId));
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

              const idx = visibleList.findIndex((c) =>
                selectedKeyResolved && selectedKey
                  ? conversationUniqueKey(c) === selectedKey
                  : c.id === selectedId,
              );
              const next =
                e.key === "ArrowDown"
                  ? Math.min(idx + 1, visibleList.length - 1)
                  : Math.max(idx - 1, 0);
              if (next !== idx && visibleList[next]) {
                selectConversation(visibleList[next]);
                const el = document.getElementById(conversationDomId(visibleList[next]));
                el?.scrollIntoView({ block: "nearest" });
              }
            }}
          >
            {loading && conversations.length === 0 ? (
              <div className="py-8 text-center text-xs text-stone-600">
                加载中...
              </div>
            ) : mainList.length === 0 ? (
              <div className="py-8 text-center text-xs text-stone-600">
                {viewMode === "conversations"
                  ? "暂无对话记录"
                  : viewMode === "jarvis"
                    ? "暂无 Jarvis 会话"
                    : viewMode === "workers"
                      ? "暂无后台工位会话"
                      : viewMode === "codex"
                        ? "暂无 Codex 会话"
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
                        className="mission-list-group-header sticky top-0 z-10 flex w-full items-center gap-1.5 border-b px-3 py-1.5 text-[11px] text-stone-500 hover:text-stone-300"
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
                              ? "text-emerald-300/80"
                              : "text-stone-500",
                          )}
                        >
                          {displayName}
                        </span>
                        {activeCount > 0 && (
                          <span className="rounded bg-emerald-400/[0.12] px-1 py-0.5 text-[9px] text-emerald-300">
                            运行中 {activeCount}
                          </span>
                        )}
                        <span className="ml-auto text-stone-600">
                          {totalCount}
                        </span>
                      </button>
                      {!isSlotCollapsed &&
                        sessions.map((conv) => {
                          const children = subagentMap.get(conv.id) || [];
                          const isExpanded = expandedParents.has(conv.id);
                          return (
                            <div key={conversationUniqueKey(conv)} id={conversationDomId(conv)}>
                              <ConversationListItem
                                conv={conv}
                                active={isConversationActive(conv)}
                                onClick={() => selectConversation(conv)}
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
                                    key={conversationUniqueKey(child)}
                                    conv={child}
                                    active={isConversationActive(child)}
                                    onClick={() => selectConversation(child)}
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
                  <div className="flex items-center gap-2 px-2 py-1.5 text-[11px] font-medium text-amber-300">
                    <span>★ 标星</span>
                    <span className="text-stone-600">{starredConvs.length}</span>
                  </div>
                  <div className="space-y-0.5">
                    {starredConvs.map((conv) =>
                      viewMode === "gemini" ? (
                        <div key={`star-${conversationUniqueKey(conv)}`} id={conversationDomId(conv)}>
                          <GeminiListItem
                            conv={conv}
                            active={isConversationActive(conv)}
                            onClick={() => selectConversation(conv)}
                            starred
                            onToggleStar={() => toggleStar(conv.id)}
                          />
                        </div>
                      ) : (
                        <div key={`star-${conversationUniqueKey(conv)}`} id={conversationDomId(conv)}>
                          <ConversationListItem
                            conv={conv}
                            active={isConversationActive(conv)}
                            onClick={() => selectConversation(conv)}
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
                      className="mission-list-group-header sticky top-0 z-10 flex w-full items-center gap-1.5 border-b px-3 py-1.5 text-[11px] text-stone-500 hover:text-stone-300"
                    >
                      {isDayCollapsed ? (
                        <ChevronRight className="w-3 h-3" />
                      ) : (
                        <ChevronDown className="w-3 h-3" />
                      )}
                      <span className="font-medium">{label}</span>
                      <span className="ml-auto text-stone-600">
                        {items.length}
                      </span>
                    </button>
                    {!isDayCollapsed &&
                      (viewMode === "gemini" ? (
                        <div className="space-y-0.5">
                          {items.map((conv) => (
                            <div key={conversationUniqueKey(conv)} id={conversationDomId(conv)}>
                              <GeminiListItem
                                conv={conv}
                                active={isConversationActive(conv)}
                                onClick={() => selectConversation(conv)}
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
                            <div key={conversationUniqueKey(conv)} id={conversationDomId(conv)}>
                              <ConversationListItem
                                conv={conv}
                                active={isConversationActive(conv)}
                                onClick={() => selectConversation(conv)}
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
                                    key={conversationUniqueKey(child)}
                                    conv={child}
                                    active={isConversationActive(child)}
                                    onClick={() => selectConversation(child)}
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
      )}

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
            <div className="mission-conversation-detail-header flex items-center gap-3 border-b px-4 py-3">
              <button
                onClick={() => {
                  setSidebarCollapsed(false);
                  setShowList(true);
                }}
                className="rounded p-1 text-stone-500 hover:bg-white/[0.04] hover:text-stone-300 md:hidden"
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
                      className="flex items-center gap-1 text-[11px] text-stone-500 transition-colors hover:text-stone-300"
                      title="返回父会话"
                    >
                      <GitBranch className="w-3 h-3" />
                      <span>子任务</span>
                    </button>
                  )}
                  <span
                    className="truncate text-sm font-medium text-stone-100"
                    title={`${conversationDetailTitle(selectedConv)}\nID: ${selectedConv.id}${selectedConv.jsonlPath ? `\nJSONL: ${selectedConv.jsonlPath}` : ""}`}
                  >
                    {conversationDetailTitle(selectedConv)}
                  </span>
                  <Badge
                    variant="outline"
                    className={cn(
                      "border-white/10 text-[10px]",
                      conversationStatusClass(selectedConv),
                    )}
                  >
                    {conversationStatusLabel(selectedConv)}
                  </Badge>
                </div>
                <div className="flex items-center gap-3 text-[11px] text-stone-500">
                  {selectedConv.messageCount > 0 && (
                    <span>{selectedConv.messageCount} 条消息</span>
                  )}
                  {selectedConv.model && (
                    <span className="font-mono">{selectedConv.model}</span>
                  )}
                  <span className="font-mono" title={selectedConv.id}>
                    ID {conversationShortId(selectedConv)}
                  </span>
                  {selectedConv.slotId && (
                    <span className="font-mono text-teal-300/60">
                      {selectedConv.slotId}
                    </span>
                  )}
                  <span>
                    {new Date(selectedConv.startedAt).toLocaleString("zh-CN")}
                  </span>
                </div>
                {selectedConv.llmSummary && (
                  <p className="mt-0.5 line-clamp-1 text-[11px] text-stone-500">
                    {conversationSecondarySummary(selectedConv, conversationTitle(selectedConv)) || selectedConv.llmSummary}
                  </p>
                )}
              </div>
              <button
                onClick={() => {
                  const next = !showLabels;
                  setShowLabels(next);
                  if (selectedId) {
                    fetchMessages(selectedId, next, { jsonlPath: selectedJsonlPath });
                  }
                }}
                className={cn(
                  "flex items-center gap-1 px-2 py-1 rounded-md text-[11px] font-medium transition-colors flex-shrink-0",
                  showLabels
                    ? "bg-amber-500/10 text-amber-300 border border-amber-500/30"
                    : "border border-white/[0.08] text-stone-500 hover:border-white/15 hover:text-stone-300",
                )}
                title="显示/隐藏标签"
              >
                <Tag className="w-3 h-3" />
                标签
              </button>
            </div>

            {/* Messages + Events Timeline */}
            <div className="mission-message-pane flex flex-1 flex-col overflow-hidden px-4 py-3">
              {loadingMessages ? (
                <div className="py-8 text-center text-xs text-stone-600">
                  加载消息...
                </div>
              ) : messages.length === 0 &&
                selectedConv.source === "router_chat" ? (
                <div className="flex items-center justify-center py-12">
                  <div className="max-w-sm text-center space-y-3">
                    <Sparkles className="mx-auto h-8 w-8 text-indigo-300/50" />
                    <div className="text-sm font-medium text-stone-400">
                      Gemini Router Chat
                    </div>
                    <div className="space-y-1 text-xs text-stone-600">
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
                    <p className="rounded border border-white/[0.08] px-3 py-2 text-[11px] text-stone-600">
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
                        className="text-[11px] text-indigo-300 transition-colors hover:text-indigo-200"
                      >
                        → 查看关联 Board 任务
                      </button>
                    )}
                  </div>
                </div>
              ) : messages.length === 0 &&
                codexToolCalls.length === 0 &&
                viewMode === "codex" &&
                selectedConv.source === "codex_cli" &&
                !selectedConv.jsonlPath ? (
                <div className="flex items-center justify-center py-12">
                  <div className="max-w-md rounded-md border border-amber-500/20 bg-amber-500/5 px-4 py-3 text-xs text-stone-500">
                    <div className="mb-1 font-medium text-amber-300">旧 PTY 占位</div>
                    <div>
                      这不是可回放的 Codex JSONL 会话。MissionD DB 中还保留着这个
                      `pty-slot-*` 诊断行，但它没有 jsonlPath，也没有消息内容；页面的
                      “进行中”现在只统计真实 JSONL 会话。
                    </div>
                    {selectedConv.slotId && (
                      <div className="mt-2 font-mono text-stone-600">{selectedConv.slotId}</div>
                    )}
                  </div>
                </div>
              ) : messages.length === 0 && codexToolCalls.length === 0 ? (
                <div className="py-8 text-center text-xs text-stone-600">
                  暂无消息
                </div>
              ) : (
                <>
                  {/* Event stats summary */}
                  {(events.length > 0 || turns.length > 0) && (
                    <div className="mb-2 flex items-center gap-3 rounded-md border border-white/[0.07] bg-white/[0.025] px-2 py-1.5 text-[10px] text-stone-600">
                      <Layers className="w-3 h-3" />
                      {events.length > 0 && <span>{events.length} 系统事件</span>}
                      {turns.length > 0 && (
                        <span className="text-teal-300/70">
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
                  {codexToolCalls.length > 0 && (
                    <div className="mb-2 flex items-center gap-3 rounded-md border border-amber-500/[0.16] bg-amber-500/[0.035] px-2 py-1.5 text-[10px] text-stone-500">
                      <Wrench className="w-3 h-3 text-amber-400" />
                      <span className="text-amber-300">{codexToolCalls.length} Codex tool calls</span>
                      {codexToolCallSource && (
                        <span className="font-mono text-stone-600">{codexToolCallSource}</span>
                      )}
                      {codexToolCallError && codexToolCallSource === "jsonl_fallback" && (
                        <span className="truncate text-stone-700" title={codexToolCallError}>
                          mission_codex_ops fallback
                        </span>
                      )}
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
                        <div className="mb-2 flex flex-wrap items-center gap-2 rounded-md border border-amber-500/[0.16] bg-amber-500/[0.035] px-2 py-1.5 text-[10px]">
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
                  <div className="relative flex flex-1 min-h-0">
                  <Virtuoso
                    ref={virtuosoRef}
                    style={{ flex: 1, minHeight: 0 }}
                    data={flatTimeline}
                    followOutput={viewMode === "codex" ? followCodexOutput : false}
                    scrollerRef={viewMode === "codex" ? handleCodexScrollerRef : undefined}
                    atBottomThreshold={CODEX_BOTTOM_THRESHOLD_PX}
                    atBottomStateChange={
                      viewMode === "codex" ? handleCodexAtBottomChange : undefined
                    }
                    rangeChanged={(range: ListRange) => {
                      visibleRangeRef.current = {
                        startIndex: range.startIndex,
                        endIndex: range.endIndex,
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
                          <div className="my-3 flex items-center gap-3">
                            <div className="h-px flex-1 bg-white/[0.07]" />
                            <span className="text-[10px] text-stone-600">
                              {item.date}
                            </span>
                            <div className="h-px flex-1 bg-white/[0.07]" />
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
                      if (item.type === "codex-tool-call") {
                        return <CodexToolCallBubble call={item.data} jsonlPath={jsonlPath} />;
                      }
                      if (item.type === "codex-tool-call-group") {
                        return (
                          <CodexToolCallGroupBubble
                            group={item.group}
                            jsonlPath={jsonlPath}
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
                  {viewMode === "codex" &&
                    flatTimeline.length > 0 &&
                    (!codexFollowLatest || !codexAtBottom) && (
                      <button
                        type="button"
                        onClick={handleFollowLatest}
                        className="absolute bottom-4 z-20 flex h-9 w-9 items-center justify-center rounded-full border border-teal-400/30 bg-neutral-950/90 text-teal-200 shadow-lg shadow-black/40 backdrop-blur transition-colors hover:border-teal-300/60 hover:bg-teal-400/10 hover:text-white"
                        style={{
                          right: userIndex.length > 0 ? "9.75rem" : "1rem",
                        }}
                        title="跟随最新消息"
                        aria-label="跟随最新消息"
                      >
                        <ChevronDown className="h-5 w-5" />
                      </button>
                    )}
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
              <MessageSquare className="mx-auto mb-2 h-8 w-8 text-stone-700" />
              <p className="text-sm text-stone-600">选择一个对话查看详情</p>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
