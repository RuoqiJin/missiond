import { NextRequest, NextResponse } from 'next/server';
import { resolveXjpcodeBaseUrl, xjpcodeUrl } from '@/lib/xjpcodeProxy';

export const runtime = 'nodejs';
export const dynamic = 'force-dynamic';
export const revalidate = 0;

async function readJson(response: Response): Promise<unknown> {
  const text = await response.text();
  if (!text.trim()) return null;
  try {
    return JSON.parse(text);
  } catch {
    return { raw: text };
  }
}

export async function GET(req: NextRequest) {
  let baseUrl: string;
  try {
    baseUrl = resolveXjpcodeBaseUrl(req.nextUrl.searchParams.get('baseUrl'));
  } catch (err) {
    return NextResponse.json({ ok: false, error: String(err) }, { status: 400 });
  }

  const health = await fetch(xjpcodeUrl(baseUrl, '/worker/v1/health'), {
    cache: 'no-store',
  }).then(async (response) => ({
    ok: response.ok,
    status: response.status,
    body: await readJson(response),
  })).catch((err) => ({
    ok: false,
    status: 0,
    body: { error: String(err) },
  }));

  const models = await fetch(xjpcodeUrl(baseUrl, '/v1/models'), {
    cache: 'no-store',
  }).then(async (response) => ({
    ok: response.ok,
    status: response.status,
    body: await readJson(response),
  })).catch((err) => ({
    ok: false,
    status: 0,
    body: { error: String(err) },
  }));

  return NextResponse.json({
    ok: health.ok || models.ok,
    baseUrl,
    health,
    models,
  });
}
