import { NextResponse } from 'next/server';
import { callTool } from '@/lib/missiond';

interface SlotInfo {
  id: string;
  role: string;
  description?: string;
}

interface PtyStatus {
  state?: string;
}

export async function GET() {
  try {
    const result = await callTool('mission_slots') as SlotInfo[];
    const filtered = result || [];

    // Check PTY status for all slots in parallel
    const statuses = await Promise.allSettled(
      filtered.map((s) =>
        callTool('mission_pty_status', { slotId: s.id })
          .then((r) => r as PtyStatus)
          .catch(() => null)
      )
    );

    const slots = filtered.map((s, i) => {
      const status = statuses[i].status === 'fulfilled' ? (statuses[i] as PromiseFulfilledResult<PtyStatus | null>).value : null;
      const running = !!status?.state && status.state !== 'exited';
      return {
        id: s.id,
        role: s.role,
        label: s.id.replace(/^slot-/, '').replace(/-\d+$/, ''),
        running,
      };
    });

    // Running slots first
    slots.sort((a, b) => (a.running === b.running ? 0 : a.running ? -1 : 1));
    return NextResponse.json(slots);
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 502 });
  }
}
