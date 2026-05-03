import { NextResponse } from 'next/server';
import { callTool } from '@/lib/missiond';

type ProjectRow = Record<string, unknown> & { id?: string };
type UniverseResponse = { services?: Array<Record<string, unknown>> };

export async function GET() {
  try {
    const listResult = await callTool('mission_project', { action: 'list' });
    const projects: ProjectRow[] = Array.isArray(listResult) ? listResult.filter(isProjectRow) : [];
    const universeResult = await callTool('mission_project', { action: 'universe' }).catch(() => null);
    const services = Array.isArray((universeResult as UniverseResponse | null)?.services)
      ? ((universeResult as UniverseResponse).services ?? [])
      : [];
    const byProject = new Map<string, Array<Record<string, unknown>>>();
    for (const service of services) {
      const project = typeof service.project === 'string' ? service.project : undefined;
      if (!project) continue;
      byProject.set(project, [...(byProject.get(project) || []), service]);
    }
    const enriched = projects.map((project) => {
      const id = typeof project.id === 'string' ? project.id : '';
      return {
        ...project,
        runtimeServices: byProject.get(id) || [],
      };
    });
    return NextResponse.json(enriched);
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 502 });
  }
}

function isProjectRow(value: unknown): value is ProjectRow {
  return !!value && typeof value === 'object';
}
