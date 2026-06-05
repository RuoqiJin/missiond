import { NextRequest, NextResponse } from 'next/server';
import { open, readFile, readdir, stat } from 'fs/promises';
import os from 'os';
import path from 'path';
import { callTool } from '@/lib/missiond';

type JsonObject = Record<string, unknown>;

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
  source: 'mission_codex_ops' | 'jsonl_fallback';
  rawCallJson?: string | null;
  rawOutputJson?: string | null;
  lineNo?: number | null;
  outputLineNo?: number | null;
}

interface CodexProjectedMessage {
  id: number;
  sessionId: string;
  role: string;
  rawRole: string | null;
  roleDisplay: string | null;
  content: string;
  rawContent: string | null;
  messageUuid: string;
  model: string | null;
  timestamp: string;
  metadata: string | null;
  toolName: string | null;
  seq: number | null;
}

interface CodexProjectedTurn {
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

interface ConversationListEntry {
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
  updatedAt?: string | null;
  endedAt: string | null;
  status: string;
  conversationType: string;
  chatType: string | null;
  llmSummary: string | null;
  labels?: [string, string][];
}

interface CodexJsonlFile {
  path: string;
  mtimeMs: number;
  size: number;
}

const CODEX_FALLBACK_SCAN_TTL_MS = 15_000;
const CODEX_FALLBACK_WINDOW_MS = 48 * 60 * 60 * 1000;
const CODEX_FALLBACK_ACTIVE_MS = 20 * 60 * 1000;
const CODEX_FALLBACK_HEAD_BYTES = 128 * 1024;
const CODEX_FALLBACK_TAIL_BYTES = 384 * 1024;

let codexJsonlFileCache:
  | { expiresAt: number; files: CodexJsonlFile[] }
  | null = null;

function asObject(value: unknown): JsonObject | null {
  return value && typeof value === 'object' && !Array.isArray(value)
    ? (value as JsonObject)
    : null;
}

function asString(value: unknown): string | null {
  return typeof value === 'string' ? value : null;
}

function asNumber(value: unknown): number | null {
  return typeof value === 'number' && Number.isFinite(value) ? value : null;
}

function codexSessionsRoot(): string {
  const codexHome = process.env.CODEX_HOME || path.join(os.homedir(), '.codex');
  return path.join(codexHome, 'sessions');
}

function codexSessionIdFromPath(jsonlPath: string): string | null {
  const match = jsonlPath.match(/([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})\.jsonl$/i);
  return match?.[1] ?? null;
}

async function readFileSlice(filePath: string, start: number, length: number): Promise<string> {
  if (length <= 0) return '';
  const handle = await open(filePath, 'r');
  try {
    const buffer = Buffer.alloc(length);
    const { bytesRead } = await handle.read(buffer, 0, length, start);
    return buffer.subarray(0, bytesRead).toString('utf8');
  } finally {
    await handle.close();
  }
}

async function collectCodexJsonlFiles(
  dir: string,
  cutoffMs: number,
  out: CodexJsonlFile[],
  depth = 0,
): Promise<void> {
  if (depth > 5) return;
  let entries: { name: string; isDirectory(): boolean; isFile(): boolean }[];
  try {
    entries = await readdir(dir, { withFileTypes: true });
  } catch {
    return;
  }

  await Promise.all(entries.map(async (entry) => {
    const fullPath = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      await collectCodexJsonlFiles(fullPath, cutoffMs, out, depth + 1);
      return;
    }
    if (!entry.isFile() || !entry.name.endsWith('.jsonl') || !entry.name.startsWith('rollout-')) {
      return;
    }
    try {
      const st = await stat(fullPath);
      if (st.mtimeMs >= cutoffMs) {
        out.push({ path: fullPath, mtimeMs: st.mtimeMs, size: st.size });
      }
    } catch {
      // Ignore files that are rotated or removed while we scan.
    }
  }));
}

async function recentCodexJsonlFiles(): Promise<CodexJsonlFile[]> {
  const now = Date.now();
  if (codexJsonlFileCache && codexJsonlFileCache.expiresAt > now) {
    return codexJsonlFileCache.files;
  }

  const files: CodexJsonlFile[] = [];
  await collectCodexJsonlFiles(
    codexSessionsRoot(),
    now - CODEX_FALLBACK_WINDOW_MS,
    files,
  );
  files.sort((a, b) => b.mtimeMs - a.mtimeMs);
  const limited = files.slice(0, 80);
  codexJsonlFileCache = {
    expiresAt: now + CODEX_FALLBACK_SCAN_TTL_MS,
    files: limited,
  };
  return limited;
}

function stringifyPayload(value: unknown): string | null {
  if (value == null) return null;
  if (typeof value === 'string') return value;
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}

function parseJsonObject(value: string | null): JsonObject | null {
  if (!value) return null;
  try {
    return asObject(JSON.parse(value));
  } catch {
    return null;
  }
}

function summarizeText(value: string | null, max = 240): string | null {
  if (!value) return null;
  const compact = value.replace(/\s+/g, ' ').trim();
  return compact.length > max ? `${compact.slice(0, max)}...` : compact;
}

function summarizeToolInput(toolName: string, rawInput: string | null): string | null {
  if (!rawInput) return null;
  try {
    const parsed = JSON.parse(rawInput) as Record<string, unknown>;
    if (typeof parsed.title === 'string') return parsed.title;
    if (typeof parsed.cmd === 'string') return parsed.cmd;
    if (typeof parsed.command === 'string') return parsed.command;
    if (typeof parsed.code === 'string') return summarizeText(parsed.code, 200);
    if (typeof parsed.ref_id === 'string') return parsed.ref_id;
    if (typeof parsed.path === 'string') return parsed.path;
    if (typeof parsed.file_path === 'string') return parsed.file_path;
    if (typeof parsed.pattern === 'string') return `${toolName}: ${parsed.pattern}`;
  } catch {
    // Fall through to compact raw preview.
  }
  return summarizeText(rawInput);
}

function toolDisplayTitle(rawInput: string | null): string | null {
  const args = parseJsonObject(rawInput);
  return asString(args?.title);
}

function normalizeCodexToolCall(value: unknown): CodexToolCall | null {
  const raw = asObject(value);
  if (!raw) return null;
  const callId = asString(raw.callId) ?? asString(raw.call_id);
  const toolName = asString(raw.toolName) ?? asString(raw.tool_name);
  const timestamp = asString(raw.timestamp);
  if (!callId || !toolName || !timestamp) return null;
  const rawInput = asString(raw.rawInput) ?? asString(raw.raw_input);
  const rawOutput = asString(raw.rawOutput) ?? asString(raw.raw_output);
  return {
    callId,
    messageId: asNumber(raw.messageId) ?? asNumber(raw.message_id),
    toolName,
    namespace: asString(raw.namespace) ?? asString(raw.server) ?? null,
    displayTitle: asString(raw.displayTitle) ?? asString(raw.display_title) ?? toolDisplayTitle(rawInput),
    inputSummary: asString(raw.inputSummary) ?? asString(raw.input_summary) ?? summarizeToolInput(toolName, rawInput),
    rawInput,
    outputSummary: asString(raw.outputSummary) ?? asString(raw.output_summary) ?? summarizeText(rawOutput),
    rawOutput,
    status: asString(raw.status) ?? 'unknown',
    durationMs: asNumber(raw.durationMs) ?? asNumber(raw.duration_ms),
    timestamp,
    source: 'mission_codex_ops',
  };
}

function durationMsFromJson(value: unknown): number | null {
  const obj = asObject(value);
  if (!obj) return null;
  const secs = asNumber(obj.secs) ?? 0;
  const nanos = asNumber(obj.nanos) ?? 0;
  const ms = secs * 1000 + nanos / 1_000_000;
  return Number.isFinite(ms) ? Math.round(ms) : null;
}

function extractToolResultContent(
  value: unknown,
  lineNo: number,
): {
  text: string | null;
  summary: string | null;
  status: string;
  images: { lineNo: number; index: number; mediaType: string }[];
} {
  const result = asObject(value);
  const ok = asObject(result?.Ok);
  const err = result?.Err;
  if (err != null) {
    const text = stringifyPayload(err);
    return { text, summary: summarizeText(text), status: 'error', images: [] };
  }
  if (!ok) {
    const text = stringifyPayload(value);
    return { text, summary: summarizeText(text), status: 'unknown', images: [] };
  }

  const content = Array.isArray(ok.content) ? ok.content : [];
  const parts: string[] = [];
  const images: { lineNo: number; index: number; mediaType: string }[] = [];
  let imageIndex = 0;
  for (const block of content) {
    const obj = asObject(block);
    if (!obj) continue;
    const type = asString(obj.type);
    if (type === 'text') {
      const text = asString(obj.text);
      if (text) parts.push(text);
      continue;
    }
    if (type === 'image') {
      const mediaType = asString(obj.mimeType) ?? asString(obj.mime_type) ?? 'image/png';
      images.push({ lineNo, index: imageIndex, mediaType });
      parts.push(`[截图 ${imageIndex + 1}: ${mediaType}]`);
      imageIndex++;
    }
  }

  const text = parts.join('\n');
  const isError = ok.isError === true || ok.is_error === true;
  return {
    text: text || stringifyPayload(ok),
    summary: summarizeText(text || stringifyPayload(ok)),
    status: isError ? 'error' : 'success',
    images,
  };
}

function extractCodexTextContent(payload: JsonObject): string {
  const content = payload.content;
  if (typeof content === 'string') return content;
  if (!Array.isArray(content)) return '';
  const parts: string[] = [];
  for (const block of content) {
    const obj = asObject(block);
    if (!obj) continue;
    const text = asString(obj.text);
    if (text) parts.push(text);
  }
  return parts.join('\n');
}

function mediaTypeFromDataUrl(value: string | null): string | null {
  if (!value) return null;
  const match = value.match(/^data:([^;,]+);base64,/);
  return match?.[1] ?? null;
}

function normalizeCodexRawContent(payload: JsonObject): { rawContent: string | null; hasImage: boolean } {
  const content = payload.content;
  if (!Array.isArray(content)) return { rawContent: null, hasImage: false };

  let hasImage = false;
  const blocks: JsonObject[] = [];
  for (const block of content) {
    const obj = asObject(block);
    if (!obj) continue;
    const blockType = asString(obj.type);
    if (blockType === 'input_text' || blockType === 'output_text' || blockType === 'text') {
      blocks.push({ type: 'text', text: asString(obj.text) ?? '' });
      continue;
    }
    if (blockType === 'input_image' || blockType === 'image') {
      hasImage = true;
      const imageUrl = asString(obj.image_url);
      const mediaType =
        mediaTypeFromDataUrl(imageUrl) ??
        asString(obj.media_type) ??
        asString(asObject(obj.source)?.media_type) ??
        'image/png';
      blocks.push({
        type: 'image',
        source: {
          media_type: mediaType,
        },
      });
    }
  }

  if (!hasImage) return { rawContent: null, hasImage: false };
  return { rawContent: JSON.stringify(blocks), hasImage };
}

function isCodexContextInjection(text: string): boolean {
  const trimmed = text.trimStart();
  return (
    trimmed.startsWith('<permissions instructions>') ||
    trimmed.startsWith('<app-context>') ||
    trimmed.startsWith('# AGENTS.md instructions') ||
    trimmed.includes('<environment_context>')
  );
}

function codexRoleDisplay(role: string, rawRole: string | null, content: string): string {
  if (role === 'system' && rawRole === 'developer') return '开发者上下文';
  if (role === 'system' && isCodexContextInjection(content)) return '运行上下文';
  if (role === 'system') return '系统上下文';
  if (role === 'user') return '用户';
  return 'AI助理';
}

function projectCodexMessageRole(rawRole: string | null, content: string): string | null {
  if (rawRole === 'assistant') return 'assistant';
  if (rawRole === 'developer' || rawRole === 'system') return 'system';
  if (rawRole === 'user') return isCodexContextInjection(content) ? 'system' : 'user';
  return null;
}

function recordTimestamp(record: JsonObject): string | null {
  return asString(record.timestamp);
}

function touchTimestamp(current: string | null, candidate: string | null, mode: 'min' | 'max'): string | null {
  if (!candidate) return current;
  if (!current) return candidate;
  const currentMs = new Date(current).getTime();
  const candidateMs = new Date(candidate).getTime();
  if (Number.isNaN(currentMs) || Number.isNaN(candidateMs)) return current;
  if (mode === 'min') return candidateMs < currentMs ? candidate : current;
  return candidateMs > currentMs ? candidate : current;
}

function updateCodexSummaryFromRecord(
  record: JsonObject,
  state: {
    sessionId: string | null;
    startedAt: string | null;
    latestAt: string | null;
    project: string | null;
    model: string | null;
    gitBranch: string | null;
    latestUserText: string | null;
    latestAssistantText: string | null;
    observedMessages: number;
  },
) {
  const timestamp = recordTimestamp(record);
  state.startedAt = touchTimestamp(state.startedAt, timestamp, 'min');
  state.latestAt = touchTimestamp(state.latestAt, timestamp, 'max');

  const payload = asObject(record.payload);
  if (!payload) return;

  if (record.type === 'session_meta') {
    state.sessionId = asString(payload.id) ?? state.sessionId;
    state.project = asString(payload.cwd) ?? state.project;
    state.model = asString(payload.model) ?? state.model;
    state.gitBranch = asString(payload.git_branch) ?? state.gitBranch;
    return;
  }

  if (record.type === 'response_item' && payload.type === 'message') {
    const rawRole = asString(payload.role);
    const content = extractCodexTextContent(payload);
    if (!content.trim() || isCodexContextInjection(content)) return;
    if (rawRole === 'user') {
      state.latestUserText = content;
      state.observedMessages++;
    } else if (rawRole === 'assistant') {
      state.latestAssistantText = content;
      state.observedMessages++;
    }
    return;
  }

  if (record.type === 'event_msg' && payload.type === 'user_message') {
    const content = asString(payload.message) ?? '';
    if (content.trim() && !isCodexContextInjection(content)) {
      state.latestUserText = content;
      state.observedMessages++;
    }
    return;
  }

  if (record.type === 'event_msg' && payload.type === 'agent_message') {
    const content = asString(payload.message) ?? '';
    if (content.trim()) {
      state.latestAssistantText = content;
      state.observedMessages++;
    }
    return;
  }

  if (record.type === 'event_msg' && payload.type === 'task_complete') {
    const content = asString(payload.last_agent_message) ?? asString(payload.message) ?? '';
    if (content.trim()) {
      state.latestAssistantText = content;
      state.observedMessages++;
    }
  }
}

async function summarizeCodexJsonlFile(file: CodexJsonlFile): Promise<ConversationListEntry | null> {
  const sessionIdFromPath = codexSessionIdFromPath(file.path);
  if (!sessionIdFromPath) return null;

  const head = await readFileSlice(
    file.path,
    0,
    Math.min(file.size, CODEX_FALLBACK_HEAD_BYTES),
  ).catch(() => '');
  const tailStart = Math.max(0, file.size - CODEX_FALLBACK_TAIL_BYTES);
  const tail = tailStart === 0
    ? ''
    : await readFileSlice(file.path, tailStart, CODEX_FALLBACK_TAIL_BYTES).catch(() => '');

  const state = {
    sessionId: sessionIdFromPath as string | null,
    startedAt: null as string | null,
    latestAt: null as string | null,
    project: null as string | null,
    model: null as string | null,
    gitBranch: null as string | null,
    latestUserText: null as string | null,
    latestAssistantText: null as string | null,
    observedMessages: 0,
  };

  const snippets = tail ? `${head}\n${tail}` : head;
  for (const line of snippets.split(/\r?\n/)) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    try {
      updateCodexSummaryFromRecord(JSON.parse(trimmed) as JsonObject, state);
    } catch {
      // Tail snippets can start mid-line; skip incomplete records.
    }
  }

  const updatedAt = new Date(file.mtimeMs).toISOString();
  const isActive = Date.now() - file.mtimeMs <= CODEX_FALLBACK_ACTIVE_MS;
  return {
    id: state.sessionId ?? sessionIdFromPath,
    project: state.project,
    slotId: null,
    source: 'codex_cli',
    model: state.model,
    gitBranch: state.gitBranch,
    jsonlPath: file.path,
    parentSessionId: null,
    taskId: null,
    messageCount: state.observedMessages,
    startedAt: state.startedAt ?? updatedAt,
    updatedAt,
    endedAt: isActive ? null : updatedAt,
    status: isActive ? 'active' : 'completed',
    conversationType: 'codex_chat',
    chatType: 'jsonl_fallback',
    llmSummary: summarizeText(state.latestUserText ?? state.latestAssistantText, 160),
    labels: [['jsonl_fallback', '1']],
  };
}

function isCodexPtyPlaceholderEntry(conv: ConversationListEntry): boolean {
  return conv.source === 'codex_cli' &&
    !conv.jsonlPath &&
    conv.id.startsWith('pty-') &&
    conv.messageCount === 0;
}

function codexSortKey(conv: ConversationListEntry): number {
  return new Date(conv.updatedAt ?? conv.startedAt).getTime();
}

function sortCodexConversations(a: ConversationListEntry, b: ConversationListEntry): number {
  const aActive = a.status === 'active' && !isCodexPtyPlaceholderEntry(a);
  const bActive = b.status === 'active' && !isCodexPtyPlaceholderEntry(b);
  if (aActive !== bActive) return aActive ? -1 : 1;

  const aPlaceholder = isCodexPtyPlaceholderEntry(a);
  const bPlaceholder = isCodexPtyPlaceholderEntry(b);
  if (aPlaceholder !== bPlaceholder) return aPlaceholder ? 1 : -1;

  return codexSortKey(b) - codexSortKey(a);
}

async function normalizeCodexConversationActivity(
  conv: ConversationListEntry,
): Promise<ConversationListEntry> {
  if (!conv.jsonlPath) return conv;
  const files = await recentCodexJsonlFiles();
  const file = files.find((candidate) => candidate.path === conv.jsonlPath);
  if (!file) return conv;
  const updatedAt = new Date(file.mtimeMs).toISOString();
  if (Date.now() - file.mtimeMs <= CODEX_FALLBACK_ACTIVE_MS) {
    return {
      ...conv,
      status: 'active',
      endedAt: null,
      updatedAt,
    };
  }
  return {
    ...conv,
    updatedAt: conv.updatedAt ?? updatedAt,
  };
}

async function mergeCodexJsonlFallback(
  rawConversations: unknown,
  options: { status?: string; project?: string; limit: number },
): Promise<ConversationListEntry[]> {
  const dbConversations = Array.isArray(rawConversations)
    ? (rawConversations as ConversationListEntry[])
    : [];
  const files = await recentCodexJsonlFiles();
  const recentFileByPath = new Map(files.map((file) => [file.path, file]));
  const normalizedDbConversations = dbConversations.map((conv) => {
    const file = conv.jsonlPath ? recentFileByPath.get(conv.jsonlPath) : null;
    if (!file) return conv;
    const updatedAt = new Date(file.mtimeMs).toISOString();
    if (Date.now() - file.mtimeMs <= CODEX_FALLBACK_ACTIVE_MS) {
      return {
        ...conv,
        status: 'active',
        endedAt: null,
        updatedAt,
      };
    }
    return {
      ...conv,
      updatedAt: conv.updatedAt ?? updatedAt,
    };
  });

  const byId = new Set(normalizedDbConversations.map((conv) => conv.id));
  const byPath = new Set(
    normalizedDbConversations
      .map((conv) => conv.jsonlPath)
      .filter((value): value is string => Boolean(value)),
  );

  const fallback = await Promise.all(files.map(summarizeCodexJsonlFile));
  const merged = [...normalizedDbConversations];
  for (const conv of fallback) {
    if (!conv) continue;
    if (byId.has(conv.id) || byPath.has(conv.jsonlPath || '')) continue;
    if (options.project && conv.project !== options.project) continue;
    if (options.status && conv.status !== options.status) continue;
    merged.push(conv);
  }

  return merged
    .sort(sortCodexConversations)
    .slice(0, Math.max(1, options.limit));
}

async function loadFilesystemCodexConversation(sessionId: string): Promise<ConversationListEntry | null> {
  const files = await recentCodexJsonlFiles();
  const file = files.find((candidate) => codexSessionIdFromPath(candidate.path) === sessionId);
  if (!file) return null;
  return summarizeCodexJsonlFile(file);
}

function codexProjectedMessagePriority(origin: string): number {
  if (origin === 'response_item.message') return 3;
  if (origin === 'event_msg.agent_message') return 2;
  if (origin === 'event_msg.task_complete') return 1;
  return 0;
}

function timestampsWithinProjectionWindow(a: string, b: string): boolean {
  const at = new Date(a).getTime();
  const bt = new Date(b).getTime();
  if (Number.isNaN(at) || Number.isNaN(bt)) return true;
  return Math.abs(at - bt) <= 5000;
}

function pushCodexProjectedMessageDeduped(
  messages: CodexProjectedMessage[],
  message: CodexProjectedMessage,
) {
  const meta = asObject(message.metadata ? JSON.parse(message.metadata) : null);
  const origin = asString(meta?.origin) ?? '';
  for (let i = messages.length - 1; i >= 0; i--) {
    const existing = messages[i];
    if (existing.role !== message.role || existing.content !== message.content) continue;
    if (!timestampsWithinProjectionWindow(existing.timestamp, message.timestamp)) continue;
    const existingMeta = asObject(existing.metadata ? JSON.parse(existing.metadata) : null);
    const existingOrigin = asString(existingMeta?.origin) ?? '';
    if (codexProjectedMessagePriority(origin) > codexProjectedMessagePriority(existingOrigin)) {
      messages[i] = message;
    }
    return;
  }
  messages.push(message);
}

async function parseCodexMessagesFromJsonl(
  sessionId: string,
  jsonlPath: string,
): Promise<CodexProjectedMessage[]> {
  const text = await readFile(jsonlPath, 'utf8');
  const messages: CodexProjectedMessage[] = [];
  const lines = text.split(/\r?\n/);

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i].trim();
    if (!line) continue;
    let record: JsonObject;
    try {
      record = JSON.parse(line) as JsonObject;
    } catch {
      continue;
    }
    const timestamp = asString(record.timestamp) ?? new Date(0).toISOString();
    const payload = asObject(record.payload);
    if (!payload) continue;

    let rawRole: string | null = null;
    let role: string | null = null;
    let content = '';
    let origin = '';
    let rawContent: string | null = null;
    let hasImage = false;

    if (record.type === 'response_item' && payload.type === 'message') {
      rawRole = asString(payload.role);
      content = extractCodexTextContent(payload);
      const normalized = normalizeCodexRawContent(payload);
      rawContent = normalized.rawContent;
      hasImage = normalized.hasImage;
      if (!content.trim() && hasImage) content = '[图片]';
      role = projectCodexMessageRole(rawRole, content);
      origin = 'response_item.message';
    } else if (record.type === 'event_msg' && payload.type === 'user_message') {
      rawRole = 'user';
      content = asString(payload.message) ?? '';
      role = 'user';
      origin = 'event_msg.user_message';
    } else if (record.type === 'event_msg' && payload.type === 'agent_message') {
      rawRole = 'assistant';
      content = asString(payload.message) ?? '';
      role = 'assistant';
      origin = 'event_msg.agent_message';
    } else if (record.type === 'event_msg' && payload.type === 'task_complete') {
      rawRole = 'assistant';
      content = asString(payload.last_agent_message) ?? asString(payload.message) ?? '';
      role = 'assistant';
      origin = 'event_msg.task_complete';
    }

    if (!role || (!content.trim() && !hasImage)) continue;
    const lineNo = i + 1;
    pushCodexProjectedMessageDeduped(messages, {
      id: lineNo,
      sessionId,
      role,
      rawRole,
      roleDisplay: codexRoleDisplay(role, rawRole, content),
      content,
      rawContent,
      messageUuid: `codex-jsonl:${sessionId}:line-${lineNo}`,
      model: null,
      timestamp,
      metadata: JSON.stringify({
        source: 'codex_jsonl',
        origin,
        jsonlLine: lineNo,
        rawRole,
        hasImage,
      }),
      toolName: null,
      seq: lineNo,
    });
  }

  return messages.sort((a, b) => a.timestamp.localeCompare(b.timestamp) || (a.seq ?? 0) - (b.seq ?? 0));
}

async function parseCodexTurnsFromJsonl(
  sessionId: string,
  jsonlPath: string,
  messages: CodexProjectedMessage[],
): Promise<CodexProjectedTurn[]> {
  const text = await readFile(jsonlPath, 'utf8');
  const lines = text.split(/\r?\n/);
  const messageByLine = new Map(messages.map((message) => [message.id, message]));
  const turns: CodexProjectedTurn[] = [];
  let current: {
    turnIdx: number;
    startMessageId: number | null;
    endMessageId: number | null;
    userContent: string | null;
    toolNames: Set<string>;
    toolCallIds: Set<string>;
    messageCount: number;
    hasCodeChange: boolean;
    hasMcpCall: boolean;
    startedAt: string | null;
    endedAt: string | null;
  } | null = null;

  const finishTurn = () => {
    if (!current || current.startMessageId == null || current.endMessageId == null) return;
    turns.push({
      turnIdx: current.turnIdx,
      startMessageId: current.startMessageId,
      endMessageId: current.endMessageId,
      userContent: current.userContent,
      toolNames: Array.from(current.toolNames).join(',') || null,
      toolCallCount: current.toolCallIds.size,
      messageCount: current.messageCount,
      hasCodeChange: current.hasCodeChange,
      hasMcpCall: current.hasMcpCall,
      startedAt: current.startedAt,
      endedAt: current.endedAt,
      topic: current.userContent ? summarizeText(current.userContent, 80) : null,
    });
  };

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i].trim();
    if (!line) continue;
    let record: JsonObject;
    try {
      record = JSON.parse(line) as JsonObject;
    } catch {
      continue;
    }
    const payload = asObject(record.payload);
    const timestamp = asString(record.timestamp) ?? new Date(0).toISOString();
    const lineNo = i + 1;

    if (record.type === 'event_msg' && payload?.type === 'task_started') {
      finishTurn();
      current = {
        turnIdx: turns.length + 1,
        startMessageId: null,
        endMessageId: null,
        userContent: null,
        toolNames: new Set(),
        toolCallIds: new Set(),
        messageCount: 0,
        hasCodeChange: false,
        hasMcpCall: false,
        startedAt: timestamp,
        endedAt: null,
      };
      continue;
    }

    if (!current) continue;

    const projectedMessage = messageByLine.get(lineNo);
    if (projectedMessage) {
      current.messageCount++;
      current.endMessageId = lineNo;
      if (
        current.startMessageId == null &&
        projectedMessage.role === 'user' &&
        !isCodexContextInjection(projectedMessage.content)
      ) {
        current.startMessageId = lineNo;
        current.userContent = projectedMessage.content;
      }
    }

    if (record.type === 'response_item' && payload?.type === 'function_call') {
      const callId = asString(payload.call_id) ?? `${sessionId}:${lineNo}`;
      const toolName = asString(payload.name) ?? 'unknown';
      current.toolCallIds.add(callId);
      current.toolNames.add(toolName);
      if (toolName === 'apply_patch' || toolName.includes('edit')) {
        current.hasCodeChange = true;
      }
    } else if (record.type === 'event_msg') {
      const eventType = asString(payload?.type);
      if (eventType === 'mcp_tool_call_end') {
        const callId = asString(payload?.call_id) ?? `${sessionId}:${lineNo}`;
        const invocation = asObject(payload?.invocation);
        const toolName = asString(invocation?.tool) ?? 'unknown';
        current.toolCallIds.add(callId);
        current.toolNames.add(toolName);
        current.hasMcpCall = true;
      } else if (eventType === 'turn_aborted' || eventType === 'task_complete') {
        current.endedAt = timestamp;
        finishTurn();
        current = null;
      }
    }
  }

  finishTurn();
  return turns;
}

async function parseCodexToolCallsFromJsonl(jsonlPath: string, limit: number): Promise<CodexToolCall[]> {
  const text = await readFile(jsonlPath, 'utf8');
  const calls = new Map<string, CodexToolCall>();
  const lines = text.split(/\r?\n/);

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i].trim();
    if (!line) continue;
    let record: JsonObject;
    try {
      record = JSON.parse(line) as JsonObject;
    } catch {
      continue;
    }
    if (record.type !== 'response_item') continue;
    const payload = asObject(record.payload);
    if (!payload) continue;
    const payloadType = asString(payload.type);
    const timestamp = asString(record.timestamp) ?? new Date(0).toISOString();

    if (payloadType === 'function_call') {
      const callId = asString(payload.call_id);
      const toolName = asString(payload.name);
      if (!callId || !toolName) continue;
      const rawInput = stringifyPayload(payload.arguments);
      const existing = calls.get(callId);
      calls.set(callId, {
        callId,
        messageId: null,
        toolName,
        namespace: asString(payload.namespace),
        displayTitle: toolDisplayTitle(rawInput),
        inputSummary: summarizeToolInput(toolName, rawInput),
        rawInput,
        outputSummary: existing?.outputSummary ?? null,
        rawOutput: existing?.rawOutput ?? null,
        outputImages: existing?.outputImages ?? [],
        status: existing?.status && existing.status !== 'pending' ? existing.status : 'pending',
        durationMs: null,
        timestamp,
        outputTimestamp: existing?.outputTimestamp ?? null,
        source: 'jsonl_fallback',
        rawCallJson: JSON.stringify(record, null, 2),
        rawOutputJson: existing?.rawOutputJson ?? null,
        lineNo: i + 1,
        outputLineNo: existing?.outputLineNo ?? null,
      });
    } else if (payloadType === 'function_call_output') {
      const callId = asString(payload.call_id);
      if (!callId) continue;
      const existing = calls.get(callId);
      const rawOutput = stringifyPayload(payload.output);
      const status = rawOutput?.includes('Process exited with code 0') === false &&
        rawOutput?.match(/Process exited with code [1-9]/)
        ? 'error'
        : 'success';
      calls.set(callId, {
        callId,
        messageId: existing?.messageId ?? null,
        toolName: existing?.toolName ?? 'unknown',
        namespace: existing?.namespace ?? null,
        displayTitle: existing?.displayTitle ?? null,
        inputSummary: existing?.inputSummary ?? null,
        rawInput: existing?.rawInput ?? null,
        outputSummary: summarizeText(rawOutput),
        rawOutput,
        outputImages: existing?.outputImages ?? [],
        status,
        durationMs: existing?.durationMs ?? null,
        timestamp: existing?.timestamp ?? timestamp,
        outputTimestamp: timestamp,
        source: 'jsonl_fallback',
        rawCallJson: existing?.rawCallJson ?? null,
        rawOutputJson: JSON.stringify(record, null, 2),
        lineNo: existing?.lineNo ?? null,
        outputLineNo: i + 1,
      });
    } else {
      continue;
    }

    continue;
  }

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i].trim();
    if (!line) continue;
    let record: JsonObject;
    try {
      record = JSON.parse(line) as JsonObject;
    } catch {
      continue;
    }
    if (record.type !== 'event_msg') continue;
    const payload = asObject(record.payload);
    if (!payload || payload.type !== 'mcp_tool_call_end') continue;
    const timestamp = asString(record.timestamp) ?? new Date(0).toISOString();
    const callId = asString(payload.call_id);
    if (!callId) continue;
    const existing = calls.get(callId);
    const invocation = asObject(payload.invocation);
    const invocationArgs = asObject(invocation?.arguments);
    const rawInput = stringifyPayload(invocationArgs) ?? existing?.rawInput ?? null;
    const resultContent = extractToolResultContent(payload.result, i + 1);
    const toolName = asString(invocation?.tool) ?? existing?.toolName ?? 'unknown';
    calls.set(callId, {
      callId,
      messageId: existing?.messageId ?? null,
      toolName,
      namespace: asString(invocation?.server) ?? existing?.namespace ?? null,
      displayTitle: asString(invocationArgs?.title) ?? existing?.displayTitle ?? toolDisplayTitle(rawInput),
      inputSummary: summarizeToolInput(toolName, rawInput) ?? existing?.inputSummary ?? null,
      rawInput,
      outputSummary: resultContent.summary,
      rawOutput: resultContent.text,
      outputImages: resultContent.images.length > 0 ? resultContent.images : existing?.outputImages ?? [],
      status: resultContent.status,
      durationMs: durationMsFromJson(payload.duration) ?? existing?.durationMs ?? null,
      timestamp: existing?.timestamp ?? timestamp,
      outputTimestamp: timestamp,
      source: 'jsonl_fallback',
      rawCallJson: existing?.rawCallJson ?? null,
      rawOutputJson: JSON.stringify({
        timestamp: record.timestamp,
        type: record.type,
        payload: {
          type: payload.type,
          call_id: payload.call_id,
          invocation: payload.invocation,
          duration: payload.duration,
          result: resultContent.images.length > 0
            ? { note: 'image payload redacted; use outputImages refs', text: resultContent.text }
            : payload.result,
        },
      }, null, 2),
      lineNo: existing?.lineNo ?? null,
      outputLineNo: i + 1,
    });
  }

  return Array.from(calls.values())
    .sort((a, b) => a.timestamp.localeCompare(b.timestamp))
    .slice(-Math.max(0, limit));
}

async function loadCodexToolCalls(sessionId: string, jsonlPath: string | null, limit: number) {
  let codexOpsError: string | null = null;
  try {
    const result = await callTool('mission_codex_ops', {
      action: 'thread',
      threadId: sessionId,
      limit,
      includeRaw: true,
    }) as JsonObject;
    const rawCalls = Array.isArray(result.tool_calls) ? result.tool_calls : [];
    const toolCalls = rawCalls
      .map(normalizeCodexToolCall)
      .filter((call): call is CodexToolCall => call != null);
    return {
      codexToolCalls: toolCalls,
      codexToolCallSource: 'mission_codex_ops',
      codexToolCallCount: asNumber(result.total_tool_calls) ?? toolCalls.length,
      codexToolCallError: null,
    };
  } catch (err) {
    codexOpsError = String(err);
  }

  if (!jsonlPath) {
    return {
      codexToolCalls: [],
      codexToolCallSource: 'none',
      codexToolCallCount: 0,
      codexToolCallError: codexOpsError,
    };
  }

  try {
    const toolCalls = await parseCodexToolCallsFromJsonl(jsonlPath, limit);
    return {
      codexToolCalls: toolCalls,
      codexToolCallSource: 'jsonl_fallback',
      codexToolCallCount: toolCalls.length,
      codexToolCallError: codexOpsError,
    };
  } catch (err) {
    return {
      codexToolCalls: [],
      codexToolCallSource: 'none',
      codexToolCallCount: 0,
      codexToolCallError: codexOpsError ? `${codexOpsError}; fallback: ${String(err)}` : String(err),
    };
  }
}

export async function GET(req: NextRequest) {
  try {
    const sessionId = req.nextUrl.searchParams.get('sessionId');
    const search = req.nextUrl.searchParams.get('search');

    // Get messages for a specific conversation (userIndex embedded in response)
    if (sessionId) {
      const tail = req.nextUrl.searchParams.get('tail') || '200';
      const sinceId = req.nextUrl.searchParams.get('sinceId');
      const includeLabels = req.nextUrl.searchParams.get('labels') === '1';
      const includeCodexToolCalls = req.nextUrl.searchParams.get('includeCodexToolCalls') === '1';
      const toolLimit = Number(req.nextUrl.searchParams.get('toolLimit') || '100000');
      // Fetch messages and events in parallel
      let msgResult: unknown;
      let eventsResult: unknown;
      try {
        [msgResult, eventsResult] = await Promise.all([
          callTool('mission_conversation_get', {
            sessionId,
            tail: Number(tail),
            includeRaw: true,
            includeUserIndex: true,
            includeTurns: true,
            ...(sinceId != null && { sinceId: Number(sinceId) }),
            ...(includeLabels && { includeLabels: true }),
          }),
          callTool('mission_conversation_events', {
            sessionId,
            limit: 500,
          }).catch(() => ({ events: [] })),
        ]);
      } catch (err) {
        const filesystemConversation = await loadFilesystemCodexConversation(sessionId);
        if (!filesystemConversation) throw err;
        const result: Record<string, unknown> = {
          conversation: filesystemConversation,
          messages: [],
          events: [],
          turns: [],
          labels: {},
          userIndex: [],
        };
        if (includeCodexToolCalls && filesystemConversation.jsonlPath) {
          const codexMessages = await parseCodexMessagesFromJsonl(
            sessionId,
            filesystemConversation.jsonlPath,
          );
          result.codexMessages = codexMessages;
          result.turns = await parseCodexTurnsFromJsonl(
            sessionId,
            filesystemConversation.jsonlPath,
            codexMessages,
          );
          Object.assign(
            result,
            await loadCodexToolCalls(sessionId, filesystemConversation.jsonlPath, toolLimit),
          );
        }
        return NextResponse.json(result);
      }
      const result: Record<string, unknown> = { ...(msgResult as Record<string, unknown>), events: (eventsResult as Record<string, unknown>)?.events || [] };
      if (includeCodexToolCalls) {
        const conversation = asObject(result.conversation);
        let jsonlPath = asString(conversation?.jsonlPath);
        if (!jsonlPath) {
          const filesystemConversation = await loadFilesystemCodexConversation(sessionId);
          if (filesystemConversation?.jsonlPath) {
            jsonlPath = filesystemConversation.jsonlPath;
            result.conversation = {
              ...(conversation ?? {}),
              ...filesystemConversation,
            };
          }
        } else if (conversation && asString(conversation.source) === 'codex_cli') {
          result.conversation = await normalizeCodexConversationActivity({
            id: asString(conversation.id) ?? sessionId,
            project: asString(conversation.project),
            slotId: asString(conversation.slotId),
            source: asString(conversation.source) ?? 'codex_cli',
            model: asString(conversation.model),
            gitBranch: asString(conversation.gitBranch),
            jsonlPath,
            parentSessionId: asString(conversation.parentSessionId),
            taskId: asString(conversation.taskId),
            messageCount: asNumber(conversation.messageCount) ?? 0,
            startedAt: asString(conversation.startedAt) ?? new Date(0).toISOString(),
            updatedAt: asString(conversation.updatedAt),
            endedAt: asString(conversation.endedAt),
            status: asString(conversation.status) ?? 'completed',
            conversationType: asString(conversation.conversationType) ?? 'codex_chat',
            chatType: asString(conversation.chatType),
            llmSummary: asString(conversation.llmSummary),
            labels: Array.isArray(conversation.labels)
              ? (conversation.labels as [string, string][])
              : undefined,
          });
        }
        if (jsonlPath) {
          const codexMessages = await parseCodexMessagesFromJsonl(sessionId, jsonlPath);
          result.codexMessages = codexMessages;
          result.turns = await parseCodexTurnsFromJsonl(sessionId, jsonlPath, codexMessages);
        }
        Object.assign(result, await loadCodexToolCalls(sessionId, jsonlPath, toolLimit));
      }
      return NextResponse.json(result);
    }

    // Search messages
    if (search) {
      const limit = req.nextUrl.searchParams.get('limit') || '30';
      const conversationType = req.nextUrl.searchParams.get('conversationType') || undefined;
      const args: Record<string, unknown> = { query: search, limit: Number(limit) };
      if (conversationType && conversationType !== 'all') args.conversationType = conversationType;
      const result = await callTool('mission_conversation_search', args);
      return NextResponse.json(result);
    }

    // List conversations — server-side filtering by conversationType + source + project
    const status = req.nextUrl.searchParams.get('status') || undefined;
    const limit = req.nextUrl.searchParams.get('limit') || '50';
    const conversationType = req.nextUrl.searchParams.get('conversationType') || undefined;
    const source = req.nextUrl.searchParams.get('source') || undefined;
    const project = req.nextUrl.searchParams.get('project') || undefined;
    const args: Record<string, unknown> = { limit: Number(limit) };
    if (status) args.status = status;
    if (conversationType) args.conversationType = conversationType;
    if (source) args.source = source;
    if (project) args.project = project;
    const conversations = await callTool('mission_conversation_list', args);
    if (source === 'codex_cli') {
      const merged = await mergeCodexJsonlFallback(conversations, {
        status,
        project,
        limit: Number(limit),
      });
      return NextResponse.json(merged);
    }
    return NextResponse.json(conversations);
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 502 });
  }
}

export async function POST(req: NextRequest) {
  try {
    const body = await req.json();
    const { action, sessionId, label, value } = body;

    if (action === 'set_label' && sessionId && label) {
      const result = await callTool('mission_conversation_set_label', { sessionId, label, value: value ?? '1' });
      return NextResponse.json(result);
    }
    if (action === 'delete_label' && sessionId && label) {
      const result = await callTool('mission_conversation_delete_label', { sessionId, label });
      return NextResponse.json(result);
    }
    return NextResponse.json({ error: 'Unknown action' }, { status: 400 });
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 502 });
  }
}
