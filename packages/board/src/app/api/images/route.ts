import { NextRequest, NextResponse } from 'next/server';
import { readFile, readdir } from 'fs/promises';
import { join } from 'path';
import { homedir } from 'os';
import { existsSync } from 'fs';

function getImageCacheDir(): string {
  const missiondHome = process.env.MISSIOND_HOME
    || process.env.XJP_MISSION_HOME
    || (existsSync(join(homedir(), '.missiond')) ? join(homedir(), '.missiond') : join(homedir(), '.xjp-mission'));
  return join(missiondHome, 'cache', 'images');
}

const MIME: Record<string, string> = {
  png: 'image/png',
  jpg: 'image/jpeg',
  jpeg: 'image/jpeg',
  gif: 'image/gif',
  webp: 'image/webp',
};

/**
 * GET /api/images?hash=<sha256_hash>
 * Serves cached vision images by hash from ~/.missiond/cache/images/
 */
export async function GET(req: NextRequest) {
  const hash = req.nextUrl.searchParams.get('hash');
  if (!hash || !/^[a-f0-9]{16,128}$/.test(hash)) {
    return NextResponse.json({ error: 'Invalid hash' }, { status: 400 });
  }

  const dir = getImageCacheDir();

  try {
    // Find file matching the hash (hash.ext)
    const files = await readdir(dir);
    const match = files.find(f => f.startsWith(hash));
    if (!match) {
      return NextResponse.json({ error: 'Image not found' }, { status: 404 });
    }

    const ext = match.split('.').pop() || 'png';
    const buffer = await readFile(join(dir, match));

    return new NextResponse(buffer, {
      headers: {
        'Content-Type': MIME[ext] || 'image/png',
        'Cache-Control': 'public, max-age=31536000, immutable',
      },
    });
  } catch {
    return NextResponse.json({ error: 'Image not found' }, { status: 404 });
  }
}
