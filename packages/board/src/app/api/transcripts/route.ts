import { NextRequest, NextResponse } from 'next/server';
import { readdir, readFile, stat } from 'fs/promises';
import { join, basename, extname } from 'path';
import { homedir } from 'os';

// Directories to scan for MD transcript files
const SCAN_DIRS = [
  join(homedir(), 'Desktop'),
  // The project root — convention: conversation exports live here
  process.env.MISSION_PROJECT_ROOT || join(homedir(), 'Projects/missiond'),
];

// Only allow reading from approved directories
function isAllowedPath(filePath: string): boolean {
  return SCAN_DIRS.some((dir) => filePath.startsWith(dir + '/'));
}

async function listMdFiles(): Promise<{ name: string; path: string; size: number; mtime: string }[]> {
  const results: { name: string; path: string; size: number; mtime: string }[] = [];
  for (const dir of SCAN_DIRS) {
    try {
      const entries = await readdir(dir);
      for (const entry of entries) {
        if (extname(entry).toLowerCase() !== '.md') continue;
        const fullPath = join(dir, entry);
        try {
          const s = await stat(fullPath);
          if (!s.isFile()) continue;
          results.push({
            name: basename(entry, '.md'),
            path: fullPath,
            size: s.size,
            mtime: s.mtime.toISOString(),
          });
        } catch {
          // skip unreadable files
        }
      }
    } catch {
      // directory may not exist
    }
  }
  // Sort by mtime desc
  results.sort((a, b) => new Date(b.mtime).getTime() - new Date(a.mtime).getTime());
  return results;
}

export async function GET(req: NextRequest) {
  try {
    const filePath = req.nextUrl.searchParams.get('path');

    if (filePath) {
      // Read a specific file
      if (!isAllowedPath(filePath)) {
        return NextResponse.json({ error: 'Path not allowed' }, { status: 403 });
      }
      const content = await readFile(filePath, 'utf-8');
      return NextResponse.json({ path: filePath, content });
    }

    // List available files
    const files = await listMdFiles();
    return NextResponse.json({ files });
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 500 });
  }
}
