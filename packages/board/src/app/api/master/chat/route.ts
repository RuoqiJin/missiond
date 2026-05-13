import { NextRequest, NextResponse } from 'next/server';
import { randomUUID } from 'crypto';
import { callTool } from '@/lib/missiond';

type ChatPart = string | Array<{ type?: string; text?: string; image_url?: { url?: string } }>;

export async function POST(req: NextRequest) {
  try {
    const body = await req.json();
    const messages = Array.isArray(body.messages) ? body.messages : [];
    const lastUser = [...messages].reverse().find((m) => m?.role === 'user');
    const { text, imageCount } = extractContent(lastUser?.content);
    if (!text && imageCount === 0) {
      return NextResponse.json({ error: 'Missing message' }, { status: 400 });
    }

    const requestId = `req-jarvis-${randomUUID().slice(0, 8)}`;
    const requestMessage = [
      text || '(image-only message)',
      imageCount > 0
        ? `\n\n[attachments]\nimage_count=${imageCount}\nimage ingestion follow-up required.`
        : '',
    ].join('');
    let requestResult: Record<string, unknown> | null = null;
    let requestStartError: string | null = null;
    try {
      requestResult = await callTool('mission_request', {
        action: 'start',
        request_id: requestId,
        project: 'missiond',
        source: 'jarvis-master-chat',
        mode: 'human_interactive',
        compiler_mode: 'dry_run',
        review_gate_policy: 'manual',
        message: requestMessage,
        write_request_file: true,
        compat_write_file: false,
      }) as Record<string, unknown>;
    } catch (err) {
      requestStartError = String(err);
    }

    const requestArtifacts = asRecord(requestResult?.request_artifacts);
    const reviewPacket = asRecord(requestResult?.review_packet);
    const description = [
      'Jarvis master gateway objective.',
      `mission_request_id: ${requestId}`,
      requestStartError
        ? `mission_request_start_error: ${requestStartError}\n主控必须先重试 mission_request(action=start)，不要跳过 intent intake。`
        : null,
      requestArtifacts?.request_path
        ? `request_lisp: ${String(requestArtifacts.request_path)}`
        : null,
      reviewPacket?.artifact_path
        ? `intent_review_artifact: ${String(reviewPacket.artifact_path)}`
        : null,
      reviewPacket?.state ? `review_state: ${String(reviewPacket.state)}` : null,
      '',
      '用户消息：',
      text || '(image-only message)',
      '',
      imageCount > 0
        ? `图片附件：${imageCount} 张。当前网关只把图片存在前端对话 payload 中；主控应创建 follow-up 补 durable image attachment ingestion。`
        : null,
      '',
      '执行要求：resident Codex master 先按 .missiond/workflows/intent-intake-grounding.lisp 处理：第一轮理解用户想做什么；第二轮优先用 mission_context_gather 聚合 KB/SSOT/project registry/skill evidence/infra evidence 查询确认用户指的对象；第三轮生成或复用 request-local intent-alignment.lisp / work-order intent 给用户/主控确认。确认后若没有匹配 workflow.lisp，再由计划工位读取工具目录和资源能力生成 plan.lisp accepted shards；确认前不要编 plan 或派实现工位；PTY 只作为诊断。',
    ].filter(Boolean).join('\n');

    const task = await callTool('mission_board_create', {
      title: titleFromText(text, imageCount),
      description,
      category: 'infra',
      priority: 'high',
      project: 'missiond',
      autoExecute: true,
      hidden: false,
      promptTemplate: 'resident-master-control',
    }) as { id?: string };

    const taskId = task?.id || '';
    return NextResponse.json({
      ok: true,
      task_id: taskId,
      request_id: requestId,
      request_review_packet: reviewPacket || null,
      request_start_error: requestStartError,
      conversation_id: taskId ? `master:${taskId}` : null,
      message: taskId
        ? `已把这条消息交给 resident Codex master。BoardTask: ${taskId}\nmission_request: ${requestId}\n\n主控会先围绕 request-local intent artifact 做意图确认；PTY 只作为诊断视图。`
        : '已提交给 resident Codex master。',
    });
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 502 });
  }
}

function asRecord(value: unknown): Record<string, unknown> | null {
  return value && typeof value === 'object' && !Array.isArray(value)
    ? value as Record<string, unknown>
    : null;
}

function extractContent(content: ChatPart): { text: string; imageCount: number } {
  if (typeof content === 'string') return { text: content.trim(), imageCount: 0 };
  if (!Array.isArray(content)) return { text: '', imageCount: 0 };
  const text = content
    .filter((p) => p?.type === 'text' && typeof p.text === 'string')
    .map((p) => p.text?.trim())
    .filter(Boolean)
    .join('\n');
  const imageCount = content.filter((p) => p?.type === 'image_url' && p.image_url?.url).length;
  return { text, imageCount };
}

function titleFromText(text: string, imageCount: number): string {
  const clean = text.replace(/\s+/g, ' ').trim();
  if (clean) return `Jarvis -> Codex master: ${clean.slice(0, 72)}`;
  return `Jarvis -> Codex master: image objective (${imageCount})`;
}
