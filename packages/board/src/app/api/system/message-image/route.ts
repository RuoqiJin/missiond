import { NextRequest, NextResponse } from 'next/server';
import { callTool } from '@/lib/missiond';

/**
 * Serve an image from a conversation message's raw_content.
 * GET /api/system/message-image?message_id=<id>&index=<imageIndex>
 */
export async function GET(req: NextRequest) {
  const messageIdStr = req.nextUrl.searchParams.get('message_id');
  const indexStr = req.nextUrl.searchParams.get('index') || '0';

  if (!messageIdStr) {
    return NextResponse.json({ error: 'Missing message_id' }, { status: 400 });
  }

  const messageId = parseInt(messageIdStr, 10);
  const imageIndex = parseInt(indexStr, 10);

  try {
    const result = await callTool('mission_conversation_message', { message_id: messageId }) as {
      raw_content?: string;
    };

    if (!result?.raw_content) {
      return NextResponse.json({ error: 'No raw_content' }, { status: 404 });
    }

    const blocks = JSON.parse(result.raw_content);
    if (!Array.isArray(blocks)) {
      return NextResponse.json({ error: 'Invalid raw_content' }, { status: 404 });
    }

    let imgCount = 0;
    for (const block of blocks) {
      if (block.type === 'image') {
        if (imgCount === imageIndex) {
          const base64 = block.source?.data;
          const mediaType = block.source?.media_type || 'image/png';
          if (!base64) break;
          const buffer = Buffer.from(base64, 'base64');
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

    return NextResponse.json({ error: 'Image not found' }, { status: 404 });
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 500 });
  }
}
