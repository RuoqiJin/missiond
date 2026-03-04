import { NextResponse } from 'next/server';
import { callTool } from '@/lib/missiond';

interface BoardTask {
  id: string;
  title: string;
  description: string;
  status: string;
  priority: string;
  category: string;
  project?: string;
  assignee?: string;
  autoExecute: boolean;
  retryCount: number;
  maxRetries: number;
  claimExecutorId?: string;
  claimExecutorType?: string;
  claimedAt?: string;
  leaseExpiresAt?: string;
  flowPhase?: string;
  flowTemplate?: string;
  dependsOn?: string[];
  hidden: boolean;
  createdAt: string;
  updatedAt: string;
}

export async function GET() {
  try {
    // Fetch all board tasks (we'll filter client-side for deploy category)
    const allTasks = (await callTool('mission_board_list', { includeHidden: true })) as BoardTask[];
    const deployTasks = allTasks.filter((t) => t.category === 'deploy');

    // Sort by updatedAt descending (most recent first)
    deployTasks.sort((a, b) => new Date(b.updatedAt).getTime() - new Date(a.updatedAt).getTime());

    // Compute stats
    const now = new Date();
    const todayStart = new Date(now.getFullYear(), now.getMonth(), now.getDate()).toISOString();
    const todayTasks = deployTasks.filter((t) => t.updatedAt >= todayStart);

    const doneTasks = deployTasks.filter((t) => t.status === 'done');
    const failedTasks = deployTasks.filter((t) => t.status === 'failed');
    const runningTasks = deployTasks.filter((t) => t.status === 'running' || t.status === 'verifying');

    // Success rate: done / (done + failed)
    const totalFinished = doneTasks.length + failedTasks.length;
    const successRate = totalFinished > 0 ? Math.round((doneTasks.length / totalFinished) * 100) : 100;

    // Average duration for done tasks (claimedAt → updatedAt)
    const durations = doneTasks
      .filter((t) => t.claimedAt)
      .map((t) => new Date(t.updatedAt).getTime() - new Date(t.claimedAt!).getTime())
      .filter((d) => d > 0);
    const avgDurationMs = durations.length > 0 ? Math.round(durations.reduce((a, b) => a + b, 0) / durations.length) : null;

    // Retry count across all non-done tasks
    const totalRetries = deployTasks.reduce((sum, t) => sum + (t.retryCount || 0), 0);

    // Get slot status for deploy slots
    let slotStatuses: Array<{ slotId: string; state: string; taskTitle?: string; duration?: number }> = [];
    try {
      const slots = (await callTool('mission_agents')) as Array<{ slotId: string; status: string; role: string }>;
      const deploySlots = slots.filter((s) => s.role === 'deploy' || s.slotId.includes('deploy'));
      for (const slot of deploySlots) {
        const runningTask = runningTasks.find((t) => t.claimExecutorId === slot.slotId);
        const duration = runningTask?.claimedAt
          ? now.getTime() - new Date(runningTask.claimedAt).getTime()
          : undefined;
        slotStatuses.push({
          slotId: slot.slotId,
          state: slot.status,
          taskTitle: runningTask?.title,
          duration,
        });
      }
    } catch {
      // Slot info is optional
    }

    return NextResponse.json({
      stats: {
        todayCount: todayTasks.length,
        successRate,
        avgDurationMs,
        failedCount: failedTasks.length,
        runningCount: runningTasks.length,
        totalRetries,
        totalTasks: deployTasks.length,
      },
      slots: slotStatuses,
      tasks: deployTasks.slice(0, 30), // Last 30 tasks
    });
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 502 });
  }
}
