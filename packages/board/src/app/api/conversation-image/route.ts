import { NextRequest, NextResponse } from 'next/server';
import * as fs from 'fs';
import * as readline from 'readline';

function parseDataUrl(value: unknown): { buffer: Buffer; mediaType: string } | null {
  if (typeof value !== 'string') return null;
  const match = value.match(/^data:([^;,]+);base64,(.+)$/);
  if (!match) return null;
  return {
    mediaType: match[1] || 'image/png',
    buffer: Buffer.from(match[2], 'base64'),
  };
}

function imageUrlFromBlock(block: unknown): unknown {
  if (!block || typeof block !== 'object' || Array.isArray(block)) return null;
  const obj = block as Record<string, unknown>;
  const imageUrl = obj.image_url;
  if (typeof imageUrl === 'string') return imageUrl;
  if (imageUrl && typeof imageUrl === 'object' && !Array.isArray(imageUrl)) {
    return (imageUrl as Record<string, unknown>).url;
  }
  return null;
}

function toolOutputImageFromBlock(block: unknown): { buffer: Buffer; mediaType: string } | null {
  if (!block || typeof block !== 'object' || Array.isArray(block)) return null;
  const obj = block as Record<string, unknown>;
  if (obj.type !== 'image') return null;
  const dataUrl = parseDataUrl(obj.data);
  if (dataUrl) return dataUrl;
  if (typeof obj.data !== 'string') return null;
  return {
    buffer: Buffer.from(obj.data, 'base64'),
    mediaType:
      (typeof obj.mimeType === 'string' && obj.mimeType) ||
      (typeof obj.mime_type === 'string' && obj.mime_type) ||
      'image/png',
  };
}

function codexLineNoFromUuid(uuid: string): number | null {
  const match = uuid.match(/^codex-jsonl:.+:line-(\d+)$/);
  if (!match) return null;
  const lineNo = Number(match[1]);
  return Number.isFinite(lineNo) && lineNo > 0 ? lineNo : null;
}

function imageResponse(buffer: Buffer, mediaType: string) {
  const body = new ArrayBuffer(buffer.byteLength);
  new Uint8Array(body).set(buffer);
  return new NextResponse(body, {
    headers: {
      'Content-Type': mediaType,
      'Cache-Control': 'public, max-age=31536000, immutable',
    },
  });
}

/**
 * Serve images from Claude Code and Codex JSONL files.
 * GET /api/conversation-image?path=<jsonlPath>&uuid=<messageUuid>&index=<imageIndex>
 */
export async function GET(req: NextRequest) {
  const jsonlPath = req.nextUrl.searchParams.get('path');
  const uuid = req.nextUrl.searchParams.get('uuid');
  const toolLineStr = req.nextUrl.searchParams.get('toolLine');
  const indexStr = req.nextUrl.searchParams.get('index') || '0';
  const imageIndex = parseInt(indexStr, 10);
  const toolLine = toolLineStr ? parseInt(toolLineStr, 10) : null;

  if (!jsonlPath || (!uuid && !toolLine)) {
    return NextResponse.json({ error: 'Missing path and image locator' }, { status: 400 });
  }

  if (!fs.existsSync(jsonlPath)) {
    return NextResponse.json({ error: 'JSONL file not found' }, { status: 404 });
  }

  try {
    const codexLineNo = uuid ? codexLineNoFromUuid(uuid) : null;
    const fileStream = fs.createReadStream(jsonlPath, { encoding: 'utf-8' });
    const rl = readline.createInterface({ input: fileStream, crlfDelay: Infinity });
    let lineNo = 0;

    for await (const line of rl) {
      lineNo++;
      try {
        const parsed = JSON.parse(line);
        if (toolLine != null) {
          if (lineNo !== toolLine) continue;
          const content = parsed.payload?.result?.Ok?.content;
          if (!Array.isArray(content)) break;

          let imgCount = 0;
          for (const block of content) {
            const data = toolOutputImageFromBlock(block);
            if (!data) continue;
            if (imgCount === imageIndex) {
              rl.close();
              fileStream.destroy();
              return imageResponse(data.buffer, data.mediaType);
            }
            imgCount++;
          }
          break;
        }

        if (codexLineNo != null) {
          if (lineNo !== codexLineNo) continue;
          const content = parsed.payload?.content;
          if (!Array.isArray(content)) break;

          let imgCount = 0;
          for (const block of content) {
            if (block?.type === 'input_image' || block?.type === 'image') {
              if (imgCount === imageIndex) {
                const data = parseDataUrl(imageUrlFromBlock(block));
                if (!data) break;
                rl.close();
                fileStream.destroy();
                return imageResponse(data.buffer, data.mediaType);
              }
              imgCount++;
            }
          }
          break;
        }

        if (parsed.uuid !== uuid) continue;

        const content = parsed.message?.content;
        if (!Array.isArray(content)) continue;

        let imgCount = 0;
        for (const block of content) {
          if (block.type === 'image') {
            if (imgCount === imageIndex) {
              const base64 = block.source?.data;
              const mediaType = block.source?.media_type || 'image/png';
              if (!base64) break;
              const buffer = Buffer.from(base64, 'base64');
              rl.close();
              fileStream.destroy();
              return imageResponse(buffer, mediaType);
            }
            imgCount++;
          }
        }
        break; // Found message but no image at index
      } catch {
        continue; // Skip unparseable lines
      }
    }

    return NextResponse.json({ error: 'Image not found' }, { status: 404 });
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 500 });
  }
}
