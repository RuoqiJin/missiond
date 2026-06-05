import { NextRequest, NextResponse } from 'next/server';
import { readFile, readdir, stat } from 'fs/promises';
import os from 'os';
import path from 'path';
import { callTool } from '@/lib/missiond';

type JsonObject = Record<string, unknown>;

type JsonlRecord = {
  lineNo: number;
  rawLine: string;
  raw: JsonObject;
  timestamp: string | null;
  type: string | null;
  payload: JsonObject | null;
  payloadType: string | null;
  callId: string | null;
  toolName: string | null;
};

type EvidenceMessage = {
  lineNo: number;
  timestamp: string | null;
  role: string;
  rawRole: string | null;
  content: string;
  preview: string | null;
  origin: string;
};

type EvidenceToolCall = {
  callId: string;
  toolName: string;
  namespace: string | null;
  status: string;
  timestamp: string | null;
  outputTimestamp: string | null;
  lineNo: number | null;
  outputLineNo: number | null;
  rawInput: string | null;
  inputSummary: string | null;
  rawOutput: string | null;
  outputSummary: string | null;
  durationMs: number | null;
  rawCallJson: string | null;
  rawOutputJson: string | null;
};

type EvidenceTurn = {
  turnIdx: number;
  startLine: number;
  endLine: number;
  startedAt: string | null;
  endedAt: string | null;
  userContent: string | null;
  messageLines: number[];
  toolCallIds: string[];
  messageCount: number;
  toolCallCount: number;
  topic: string | null;
};

type EvidenceIndex = {
  sessionId: string;
  jsonlPath: string;
  records: JsonlRecord[];
  messages: EvidenceMessage[];
  toolCalls: EvidenceToolCall[];
  turns: EvidenceTurn[];
};

type CodexJsonlFile = {
  path: string;
  mtimeMs: number;
  size: number;
};

const CODEX_FILE_SCAN_TTL_MS = 30_000;
let codexJsonlFileCache: { expiresAt: number; files: CodexJsonlFile[] } | null = null;

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

function codexHome(): string {
  return process.env.CODEX_HOME || path.join(os.homedir(), '.codex');
}

function codexSessionsRoot(): string {
  return path.join(codexHome(), 'sessions');
}

function codexArchivedSessionsRoot(): string {
  return path.join(codexHome(), 'archived_sessions');
}

function codexSessionIdFromPath(jsonlPath: string): string | null {
  const match = jsonlPath.match(/([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})\.jsonl$/i);
  return match?.[1] ?? null;
}

function isSafeCodexJsonlPath(input: string): boolean {
  const resolved = path.resolve(input);
  const roots = [codexSessionsRoot(), codexArchivedSessionsRoot()].map((root) => path.resolve(root));
  return resolved.endsWith('.jsonl') &&
    roots.some((root) => resolved === root || resolved.startsWith(`${root}${path.sep}`));
}

async function collectJsonlFiles(dir: string, out: CodexJsonlFile[], depth = 0): Promise<void> {
  if (depth > 8) return;
  let entries: { name: string; isDirectory(): boolean; isFile(): boolean }[];
  try {
    entries = await readdir(dir, { withFileTypes: true });
  } catch {
    return;
  }

  await Promise.all(entries.map(async (entry) => {
    const fullPath = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      await collectJsonlFiles(fullPath, out, depth + 1);
      return;
    }
    if (!entry.isFile() || !entry.name.endsWith('.jsonl')) return;
    try {
      const st = await stat(fullPath);
      out.push({ path: fullPath, mtimeMs: st.mtimeMs, size: st.size });
    } catch {
      // Ignore files that are rotated while scanning.
    }
  }));
}

async function codexJsonlFiles(): Promise<CodexJsonlFile[]> {
  const now = Date.now();
  if (codexJsonlFileCache && codexJsonlFileCache.expiresAt > now) {
    return codexJsonlFileCache.files;
  }
  const files: CodexJsonlFile[] = [];
  await Promise.all([
    collectJsonlFiles(codexSessionsRoot(), files),
    collectJsonlFiles(codexArchivedSessionsRoot(), files),
  ]);
  files.sort((a, b) => b.mtimeMs - a.mtimeMs);
  codexJsonlFileCache = { expiresAt: now + CODEX_FILE_SCAN_TTL_MS, files };
  return files;
}

async function resolveJsonlPath(sessionId: string | null, explicitPath: string | null): Promise<string> {
  if (explicitPath) {
    if (!isSafeCodexJsonlPath(explicitPath)) {
      throw new Error('jsonlPath must be under Codex sessions and end with .jsonl');
    }
    return explicitPath;
  }
  if (!sessionId) throw new Error('sessionId or jsonlPath is required');

  try {
    const result = await callTool('mission_conversation_get', {
      sessionId,
      tail: 1,
      includeRaw: true,
    });
    const conversation = asObject(asObject(result)?.conversation);
    const pathFromDb = asString(conversation?.jsonlPath);
    if (pathFromDb && isSafeCodexJsonlPath(pathFromDb)) {
      return pathFromDb;
    }
  } catch {
    // Fall back to direct JSONL lookup.
  }

  const files = await codexJsonlFiles();
  const match = files.find((file) => codexSessionIdFromPath(file.path) === sessionId);
  if (!match) throw new Error(`No Codex JSONL found for session ${sessionId}`);
  return match.path;
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
  if (!compact) return null;
  return compact.length > max ? `${compact.slice(0, max)}...` : compact;
}

function extractTextContent(payload: JsonObject): string {
  const content = payload.content;
  if (typeof content === 'string') return content;
  if (!Array.isArray(content)) return '';
  const parts: string[] = [];
  for (const block of content) {
    const obj = asObject(block);
    const text = asString(obj?.text);
    if (text) parts.push(text);
  }
  return parts.join('\n');
}

function isContextInjection(text: string): boolean {
  const trimmed = text.trimStart();
  return trimmed.startsWith('<permissions instructions>') ||
    trimmed.startsWith('<app-context>') ||
    trimmed.startsWith('# AGENTS.md instructions') ||
    trimmed.includes('<environment_context>');
}

function messageFromRecord(record: JsonlRecord): EvidenceMessage | null {
  const payload = record.payload;
  if (!payload) return null;

  if (record.type === 'response_item' && record.payloadType === 'message') {
    const rawRole = asString(payload.role);
    const content = extractTextContent(payload);
    if (!content.trim()) return null;
    let role: string | null = null;
    if (rawRole === 'assistant') role = 'assistant';
    if (rawRole === 'user') role = isContextInjection(content) ? 'system' : 'user';
    if (rawRole === 'developer' || rawRole === 'system') role = 'system';
    if (!role) return null;
    return {
      lineNo: record.lineNo,
      timestamp: record.timestamp,
      role,
      rawRole,
      content,
      preview: summarizeText(content, 180),
      origin: 'response_item.message',
    };
  }

  if (record.type === 'event_msg' && record.payloadType === 'user_message') {
    const content = asString(payload.message) ?? '';
    if (!content.trim()) return null;
    return {
      lineNo: record.lineNo,
      timestamp: record.timestamp,
      role: 'user',
      rawRole: 'user',
      content,
      preview: summarizeText(content, 180),
      origin: 'event_msg.user_message',
    };
  }

  if (record.type === 'event_msg' && record.payloadType === 'agent_message') {
    const content = asString(payload.message) ?? '';
    if (!content.trim()) return null;
    return {
      lineNo: record.lineNo,
      timestamp: record.timestamp,
      role: 'assistant',
      rawRole: 'assistant',
      content,
      preview: summarizeText(content, 180),
      origin: 'event_msg.agent_message',
    };
  }

  if (record.type === 'event_msg' && record.payloadType === 'task_complete') {
    const content = asString(payload.last_agent_message) ?? asString(payload.message) ?? '';
    if (!content.trim()) return null;
    return {
      lineNo: record.lineNo,
      timestamp: record.timestamp,
      role: 'assistant',
      rawRole: 'assistant',
      content,
      preview: summarizeText(content, 180),
      origin: 'event_msg.task_complete',
    };
  }

  return null;
}

function toolInputSummary(toolName: string, rawInput: string | null): string | null {
  const parsed = parseJsonObject(rawInput);
  const cmd = asString(parsed?.cmd) ?? asString(parsed?.command);
  if (cmd) return cmd;
  const title = asString(parsed?.title);
  if (title) return title;
  const code = asString(parsed?.code);
  if (code) return summarizeText(code, 220);
  const pathValue = asString(parsed?.path) ?? asString(parsed?.file_path);
  if (pathValue) return pathValue;
  const pattern = asString(parsed?.pattern);
  if (pattern) return `${toolName}: ${pattern}`;
  return summarizeText(rawInput, 220);
}

function commandFromTool(tool: EvidenceToolCall): string | null {
  const parsed = parseJsonObject(tool.rawInput);
  return asString(parsed?.cmd) ??
    asString(parsed?.command) ??
    asString(parsed?.title) ??
    summarizeText(tool.rawInput, 160);
}

function toolPurpose(tool: EvidenceToolCall): string {
  const command = `${commandFromTool(tool) ?? ''} ${tool.rawInput ?? ''}`.toLowerCase();
  if (command.includes('memory.md')) return 'memory_index_lookup';
  if (command.includes('rollout_summaries')) return 'prior_investigation_reuse';
  if (command.includes('provider-box') || command.includes('xjpcode')) return 'provider_xjpcode_boundary_lookup';
  if (command.includes('mission_task_delegate') || command.includes('calltool') || command.includes('/api/')) return 'frontend_daemon_api_lookup';
  if (command.includes('missiond-blueprint') || command.includes('.missiond/v3')) return 'ssot_contract_lookup';
  if (command.includes('package.json') || command.includes('app.tsx') || command.includes('components')) return 'board_frontend_code_lookup';
  if (tool.status === 'error') return 'failed_probe';
  return 'evidence_lookup';
}

function outputStatus(rawOutput: string | null): string {
  if (!rawOutput) return 'success';
  if (/Process exited with code [1-9]/.test(rawOutput)) return 'error';
  return 'success';
}

function extractMcpOutput(value: unknown): { text: string | null; status: string } {
  const result = asObject(value);
  const ok = asObject(result?.Ok);
  if (result?.Err != null) {
    return { text: stringifyPayload(result.Err), status: 'error' };
  }
  if (!ok) return { text: stringifyPayload(value), status: 'unknown' };
  const content = Array.isArray(ok.content) ? ok.content : [];
  const parts: string[] = [];
  for (const block of content) {
    const obj = asObject(block);
    if (!obj) continue;
    const type = asString(obj.type);
    if (type === 'text') {
      const text = asString(obj.text);
      if (text) parts.push(text);
    } else if (type === 'image') {
      parts.push('[image payload redacted]');
    }
  }
  const text = parts.join('\n') || stringifyPayload(ok);
  return { text, status: ok.isError === true || ok.is_error === true ? 'error' : 'success' };
}

function durationMsFromJson(value: unknown): number | null {
  const obj = asObject(value);
  if (!obj) return null;
  const secs = asNumber(obj.secs) ?? 0;
  const nanos = asNumber(obj.nanos) ?? 0;
  const ms = secs * 1000 + nanos / 1_000_000;
  return Number.isFinite(ms) ? Math.round(ms) : null;
}

function eventPublic(record: JsonlRecord, includeRaw: boolean): Record<string, unknown> {
  const base: Record<string, unknown> = {
    lineNo: record.lineNo,
    timestamp: record.timestamp,
    type: record.type,
    payloadType: record.payloadType,
    callId: record.callId,
    toolName: record.toolName,
  };
  if (includeRaw) {
    base.rawLine = record.rawLine;
    base.raw = record.raw;
  } else {
    base.preview = summarizeText(record.rawLine, 320);
  }
  return base;
}

function toolPublic(tool: EvidenceToolCall, includeRaw = true): Record<string, unknown> {
  const base: Record<string, unknown> = {
    callId: tool.callId,
    toolName: tool.toolName,
    namespace: tool.namespace,
    status: tool.status,
    timestamp: tool.timestamp,
    outputTimestamp: tool.outputTimestamp,
    lineNo: tool.lineNo,
    outputLineNo: tool.outputLineNo,
    inputSummary: tool.inputSummary,
    outputSummary: tool.outputSummary,
    durationMs: tool.durationMs,
    command: commandFromTool(tool),
    purpose: toolPurpose(tool),
  };
  if (includeRaw) {
    base.rawInput = tool.rawInput;
    base.rawOutput = tool.rawOutput;
    base.rawCallJson = tool.rawCallJson;
    base.rawOutputJson = tool.rawOutputJson;
  }
  return base;
}

function readRecordLine(record: JsonObject): {
  payload: JsonObject | null;
  payloadType: string | null;
  callId: string | null;
  toolName: string | null;
} {
  const payload = asObject(record.payload);
  const payloadType = asString(payload?.type);
  const callId = asString(payload?.call_id);
  let toolName = asString(payload?.name);
  if (record.type === 'event_msg' && payloadType === 'mcp_tool_call_end') {
    const invocation = asObject(payload?.invocation);
    toolName = asString(invocation?.tool) ?? toolName;
  }
  return { payload, payloadType, callId, toolName };
}

function buildTurns(records: JsonlRecord[], messages: EvidenceMessage[], tools: EvidenceToolCall[]): EvidenceTurn[] {
  const messageByLine = new Map(messages.map((message) => [message.lineNo, message]));
  const callByLine = new Map<number, EvidenceToolCall[]>();
  for (const tool of tools) {
    if (tool.lineNo == null) continue;
    const existing = callByLine.get(tool.lineNo) ?? [];
    existing.push(tool);
    callByLine.set(tool.lineNo, existing);
  }

  const turns: EvidenceTurn[] = [];
  let current: {
    turnIdx: number;
    startLine: number;
    endLine: number;
    startedAt: string | null;
    endedAt: string | null;
    userContent: string | null;
    messageLines: number[];
    toolCallIds: string[];
  } | null = null;

  const finish = () => {
    if (!current) return;
    turns.push({
      turnIdx: current.turnIdx,
      startLine: current.startLine,
      endLine: current.endLine,
      startedAt: current.startedAt,
      endedAt: current.endedAt,
      userContent: current.userContent,
      messageLines: current.messageLines,
      toolCallIds: current.toolCallIds,
      messageCount: current.messageLines.length,
      toolCallCount: current.toolCallIds.length,
      topic: summarizeText(current.userContent, 100),
    });
    current = null;
  };

  for (const record of records) {
    if (record.type === 'event_msg' && record.payloadType === 'task_started') {
      finish();
      current = {
        turnIdx: turns.length + 1,
        startLine: record.lineNo,
        endLine: record.lineNo,
        startedAt: record.timestamp,
        endedAt: null,
        userContent: null,
        messageLines: [],
        toolCallIds: [],
      };
      continue;
    }

    if (!current) {
      const message = messageByLine.get(record.lineNo);
      if (message?.role === 'user' && !isContextInjection(message.content)) {
        current = {
          turnIdx: turns.length + 1,
          startLine: record.lineNo,
          endLine: record.lineNo,
          startedAt: record.timestamp,
          endedAt: null,
          userContent: message.content,
          messageLines: [],
          toolCallIds: [],
        };
      } else {
        continue;
      }
    }

    current.endLine = record.lineNo;
    current.endedAt = record.timestamp ?? current.endedAt;

    const message = messageByLine.get(record.lineNo);
    if (message) {
      current.messageLines.push(message.lineNo);
      if (!current.userContent && message.role === 'user' && !isContextInjection(message.content)) {
        current.userContent = message.content;
      }
    }

    const calls = callByLine.get(record.lineNo) ?? [];
    for (const call of calls) {
      if (!current.toolCallIds.includes(call.callId)) {
        current.toolCallIds.push(call.callId);
      }
    }

    if (record.type === 'event_msg' && (record.payloadType === 'turn_aborted' || record.payloadType === 'task_complete')) {
      finish();
    }
  }

  finish();
  return turns;
}

async function loadEvidenceIndex(sessionIdInput: string | null, explicitPath: string | null): Promise<EvidenceIndex> {
  const jsonlPath = await resolveJsonlPath(sessionIdInput, explicitPath);
  const sessionId = sessionIdInput ?? codexSessionIdFromPath(jsonlPath) ?? path.basename(jsonlPath, '.jsonl');
  const text = await readFile(jsonlPath, 'utf8');
  const records: JsonlRecord[] = [];
  const messages: EvidenceMessage[] = [];
  const tools = new Map<string, EvidenceToolCall>();

  const lines = text.split(/\r?\n/);
  for (let i = 0; i < lines.length; i++) {
    const rawLine = lines[i];
    if (!rawLine.trim()) continue;
    let raw: JsonObject;
    try {
      raw = JSON.parse(rawLine) as JsonObject;
    } catch {
      continue;
    }
    const { payload, payloadType, callId, toolName } = readRecordLine(raw);
    const record: JsonlRecord = {
      lineNo: i + 1,
      rawLine,
      raw,
      timestamp: asString(raw.timestamp),
      type: asString(raw.type),
      payload,
      payloadType,
      callId,
      toolName,
    };
    records.push(record);

    const message = messageFromRecord(record);
    if (message) messages.push(message);

    if (record.type === 'response_item' && payloadType === 'function_call') {
      const id = asString(payload?.call_id);
      const name = asString(payload?.name);
      if (!id || !name) continue;
      const rawInput = stringifyPayload(payload?.arguments);
      const existing = tools.get(id);
      tools.set(id, {
        callId: id,
        toolName: name,
        namespace: asString(payload?.namespace),
        status: existing?.status ?? 'pending',
        timestamp: record.timestamp,
        outputTimestamp: existing?.outputTimestamp ?? null,
        lineNo: record.lineNo,
        outputLineNo: existing?.outputLineNo ?? null,
        rawInput,
        inputSummary: toolInputSummary(name, rawInput),
        rawOutput: existing?.rawOutput ?? null,
        outputSummary: existing?.outputSummary ?? null,
        durationMs: existing?.durationMs ?? null,
        rawCallJson: JSON.stringify(raw, null, 2),
        rawOutputJson: existing?.rawOutputJson ?? null,
      });
      continue;
    }

    if (record.type === 'response_item' && payloadType === 'function_call_output') {
      const id = asString(payload?.call_id);
      if (!id) continue;
      const existing = tools.get(id);
      const rawOutput = stringifyPayload(payload?.output);
      tools.set(id, {
        callId: id,
        toolName: existing?.toolName ?? 'unknown',
        namespace: existing?.namespace ?? null,
        status: outputStatus(rawOutput),
        timestamp: existing?.timestamp ?? record.timestamp,
        outputTimestamp: record.timestamp,
        lineNo: existing?.lineNo ?? null,
        outputLineNo: record.lineNo,
        rawInput: existing?.rawInput ?? null,
        inputSummary: existing?.inputSummary ?? null,
        rawOutput,
        outputSummary: summarizeText(rawOutput, 320),
        durationMs: existing?.durationMs ?? null,
        rawCallJson: existing?.rawCallJson ?? null,
        rawOutputJson: JSON.stringify(raw, null, 2),
      });
      continue;
    }

    if (record.type === 'event_msg' && payloadType === 'mcp_tool_call_end') {
      const id = asString(payload?.call_id);
      if (!id) continue;
      const invocation = asObject(payload?.invocation);
      const invocationArgs = stringifyPayload(invocation?.arguments);
      const output = extractMcpOutput(payload?.result);
      const existing = tools.get(id);
      const name = asString(invocation?.tool) ?? existing?.toolName ?? 'unknown';
      tools.set(id, {
        callId: id,
        toolName: name,
        namespace: asString(invocation?.server) ?? existing?.namespace ?? null,
        status: output.status,
        timestamp: existing?.timestamp ?? record.timestamp,
        outputTimestamp: record.timestamp,
        lineNo: existing?.lineNo ?? null,
        outputLineNo: record.lineNo,
        rawInput: existing?.rawInput ?? invocationArgs,
        inputSummary: existing?.inputSummary ?? toolInputSummary(name, invocationArgs),
        rawOutput: output.text,
        outputSummary: summarizeText(output.text, 320),
        durationMs: durationMsFromJson(payload?.duration) ?? existing?.durationMs ?? null,
        rawCallJson: existing?.rawCallJson ?? null,
        rawOutputJson: JSON.stringify({
          timestamp: raw.timestamp,
          type: raw.type,
          payload: {
            type: payload?.type,
            call_id: payload?.call_id,
            invocation,
            duration: payload?.duration,
            result: output.text,
          },
        }, null, 2),
      });
    }
  }

  const toolCalls = Array.from(tools.values()).sort((a, b) => {
    const at = a.lineNo ?? Number.MAX_SAFE_INTEGER;
    const bt = b.lineNo ?? Number.MAX_SAFE_INTEGER;
    return at - bt;
  });
  const turns = buildTurns(records, messages, toolCalls);
  return { sessionId, jsonlPath, records, messages, toolCalls, turns };
}

function lineNoFromParams(req: NextRequest): number | null {
  const value = req.nextUrl.searchParams.get('lineNo') ??
    req.nextUrl.searchParams.get('msgSeq') ??
    req.nextUrl.searchParams.get('msgId');
  const parsed = value == null ? NaN : Number(value);
  return Number.isFinite(parsed) ? parsed : null;
}

function findContainingTurn(index: EvidenceIndex, lineNo: number | null, turnIdx: number | null): EvidenceTurn | null {
  if (turnIdx != null) return index.turns.find((turn) => turn.turnIdx === turnIdx) ?? null;
  if (lineNo == null) return null;
  return index.turns.find((turn) => lineNo >= turn.startLine && lineNo <= turn.endLine) ?? null;
}

function followingToolGroup(index: EvidenceIndex, lineNo: number): EvidenceToolCall[] {
  const nextMessageLine = index.messages
    .map((message) => message.lineNo)
    .filter((candidate) => candidate > lineNo)
    .sort((a, b) => a - b)[0] ?? Number.MAX_SAFE_INTEGER;
  return index.toolCalls.filter((call) => {
    if (call.lineNo == null) return false;
    return call.lineNo > lineNo && call.lineNo < nextMessageLine;
  });
}

function turnPayload(index: EvidenceIndex, turn: EvidenceTurn | null, focusLine: number | null, includeRaw: boolean): Record<string, unknown> {
  if (!turn) return { turn: null };
  const turnCallIds = new Set(turn.toolCallIds);
  const turnMessageLines = new Set(turn.messageLines);
  const focusTools = focusLine == null ? [] : followingToolGroup(index, focusLine);
  return {
    turn,
    messages: index.messages.filter((message) => turnMessageLines.has(message.lineNo)),
    toolCalls: index.toolCalls.filter((call) => turnCallIds.has(call.callId)).map((call) => toolPublic(call, includeRaw)),
    focus: focusLine == null
      ? null
      : {
          lineNo: focusLine,
          message: index.messages.find((message) => message.lineNo === focusLine) ?? null,
          followingToolCalls: focusTools.map((call) => toolPublic(call, includeRaw)),
          toolSummary: focusTools.map((call) => toolSummary(call)),
        },
  };
}

function toolSummary(tool: EvidenceToolCall): Record<string, unknown> {
  return {
    callId: tool.callId,
    toolName: tool.toolName,
    status: tool.status,
    lineNo: tool.lineNo,
    outputLineNo: tool.outputLineNo,
    command: commandFromTool(tool),
    purpose: toolPurpose(tool),
    result: tool.outputSummary,
    durationMs: tool.durationMs,
  };
}

export async function GET(req: NextRequest) {
  try {
    const action = req.nextUrl.searchParams.get('action') ?? 'index';
    const sessionId = req.nextUrl.searchParams.get('sessionId');
    const jsonlPath = req.nextUrl.searchParams.get('jsonlPath');
    const includeRaw = req.nextUrl.searchParams.get('includeRaw') !== '0';
    const index = await loadEvidenceIndex(sessionId, jsonlPath);
    const lineNo = lineNoFromParams(req);
    const turnIdxParam = req.nextUrl.searchParams.get('turnIdx') ?? req.nextUrl.searchParams.get('turnNo');
    const turnIdx = turnIdxParam == null ? null : Number(turnIdxParam);
    const callId = req.nextUrl.searchParams.get('callId');

    if (action === 'tool' || callId) {
      if (!callId) return NextResponse.json({ error: 'callId is required' }, { status: 400 });
      const tool = index.toolCalls.find((candidate) => candidate.callId === callId);
      const turn = tool?.lineNo == null ? null : findContainingTurn(index, tool.lineNo, null);
      return NextResponse.json({
        ok: true,
        action: 'tool',
        sessionId: index.sessionId,
        jsonlPath: index.jsonlPath,
        toolCall: tool ? toolPublic(tool, includeRaw) : null,
        turn,
        around: tool?.lineNo == null
          ? []
          : index.records
              .filter((record) => record.lineNo >= Math.max(1, tool.lineNo! - 3) && record.lineNo <= tool.lineNo! + 6)
              .map((record) => eventPublic(record, includeRaw)),
      });
    }

    if (action === 'turn') {
      const parsedTurnIdx = Number.isFinite(turnIdx) ? turnIdx : null;
      const turn = findContainingTurn(index, lineNo, parsedTurnIdx);
      return NextResponse.json({
        ok: true,
        action: 'turn',
        sessionId: index.sessionId,
        jsonlPath: index.jsonlPath,
        ...turnPayload(index, turn, lineNo, includeRaw),
      });
    }

    if (action === 'message') {
      if (lineNo == null) return NextResponse.json({ error: 'lineNo/msgSeq/msgId is required' }, { status: 400 });
      const turn = findContainingTurn(index, lineNo, null);
      const message = index.messages.find((candidate) => candidate.lineNo === lineNo) ?? null;
      const tools = followingToolGroup(index, lineNo);
      return NextResponse.json({
        ok: true,
        action: 'message',
        sessionId: index.sessionId,
        jsonlPath: index.jsonlPath,
        message,
        turn,
        followingToolCalls: tools.map((tool) => toolPublic(tool, includeRaw)),
        toolSummary: tools.map((tool) => toolSummary(tool)),
      });
    }

    if (action === 'around') {
      if (lineNo == null) return NextResponse.json({ error: 'lineNo/msgSeq/msgId is required' }, { status: 400 });
      const before = Math.max(0, Number(req.nextUrl.searchParams.get('before') ?? '8'));
      const after = Math.max(0, Number(req.nextUrl.searchParams.get('after') ?? '16'));
      return NextResponse.json({
        ok: true,
        action: 'around',
        sessionId: index.sessionId,
        jsonlPath: index.jsonlPath,
        anchorLine: lineNo,
        events: index.records
          .filter((record) => record.lineNo >= Math.max(1, lineNo - before) && record.lineNo <= lineNo + after)
          .map((record) => eventPublic(record, includeRaw)),
      });
    }

    if (action === 'events') {
      const type = req.nextUrl.searchParams.get('type');
      const payloadType = req.nextUrl.searchParams.get('payloadType');
      const fromLine = Number(req.nextUrl.searchParams.get('fromLine') ?? '1');
      const toLine = Number(req.nextUrl.searchParams.get('toLine') ?? String(Number.MAX_SAFE_INTEGER));
      const limit = Math.max(1, Math.min(500, Number(req.nextUrl.searchParams.get('limit') ?? '100')));
      const since = req.nextUrl.searchParams.get('since');
      const until = req.nextUrl.searchParams.get('until');
      const events = index.records.filter((record) => {
        if (record.lineNo < fromLine || record.lineNo > toLine) return false;
        if (type && record.type !== type) return false;
        if (payloadType && record.payloadType !== payloadType) return false;
        if (callId && record.callId !== callId) return false;
        if (since && record.timestamp && record.timestamp < since) return false;
        if (until && record.timestamp && record.timestamp > until) return false;
        return true;
      }).slice(0, limit);
      return NextResponse.json({
        ok: true,
        action: 'events',
        sessionId: index.sessionId,
        jsonlPath: index.jsonlPath,
        count: events.length,
        events: events.map((record) => eventPublic(record, includeRaw)),
      });
    }

    if (action === 'tool-summary') {
      const tools = lineNo == null
        ? index.toolCalls
        : followingToolGroup(index, lineNo);
      return NextResponse.json({
        ok: true,
        action: 'tool-summary',
        sessionId: index.sessionId,
        jsonlPath: index.jsonlPath,
        anchorLine: lineNo,
        count: tools.length,
        toolSummary: tools.map((tool) => toolSummary(tool)),
      });
    }

    return NextResponse.json({
      ok: true,
      action: 'index',
      sessionId: index.sessionId,
      jsonlPath: index.jsonlPath,
      counts: {
        records: index.records.length,
        messages: index.messages.length,
        toolCalls: index.toolCalls.length,
        turns: index.turns.length,
      },
      turns: index.turns,
      toolCalls: index.toolCalls.map((tool) => toolPublic(tool, false)),
    });
  } catch (err) {
    return NextResponse.json({ ok: false, error: String(err) }, { status: 502 });
  }
}
