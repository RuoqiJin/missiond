import { NextRequest, NextResponse } from 'next/server';
import * as fs from 'fs';
import * as readline from 'readline';

/**
 * Serve images from Claude Code JSONL files.
 * GET /api/conversation-image?path=<jsonlPath>&uuid=<messageUuid>&index=<imageIndex>
 */
export async function GET(req: NextRequest) {
  const jsonlPath = req.nextUrl.searchParams.get('path');
  const uuid = req.nextUrl.searchParams.get('uuid');
  const indexStr = req.nextUrl.searchParams.get('index') || '0';
  const imageIndex = parseInt(indexStr, 10);

  if (!jsonlPath || !uuid) {
    return NextResponse.json({ error: 'Missing path or uuid' }, { status: 400 });
  }

  if (!fs.existsSync(jsonlPath)) {
    return NextResponse.json({ error: 'JSONL file not found' }, { status: 404 });
  }

  try {
    const fileStream = fs.createReadStream(jsonlPath, { encoding: 'utf-8' });
    const rl = readline.createInterface({ input: fileStream, crlfDelay: Infinity });

    for await (const line of rl) {
      try {
        const parsed = JSON.parse(line);
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
              return new NextResponse(buffer, {
                headers: {
                  'Content-Type': mediaType,
                  'Cache-Control': 'public, max-age=31536000, immutable',
                },
              });
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
