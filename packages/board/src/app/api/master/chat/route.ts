import { NextRequest, NextResponse } from 'next/server';
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

    const description = [
      'Jarvis master gateway objective.',
      '',
      '用户消息：',
      text || '(image-only message)',
      '',
      imageCount > 0
        ? `图片附件：${imageCount} 张。当前网关只把图片存在前端对话 payload 中；主控应创建 follow-up 补 durable image attachment ingestion。`
        : null,
      '',
      '执行要求：resident Codex master 先读取 Lisp SSOT、Board、KB、event/conversation evidence，再决定是否派工位。不要直接依赖 PTY 文本关闭任务。',
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
      conversation_id: taskId ? `master:${taskId}` : null,
      message: taskId
        ? `已把这条消息交给 resident Codex master。BoardTask: ${taskId}\n\n主控会从 durable Board/KB/Lisp/事件证据推进；PTY 只作为诊断视图。`
        : '已提交给 resident Codex master。',
    });
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 502 });
  }
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
