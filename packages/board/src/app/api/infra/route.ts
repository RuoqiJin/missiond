import { NextResponse } from 'next/server';
import { callTool } from '@/lib/missiond';

export async function GET() {
  try {
    const [targets, health, skillEvidence, credentialRefs, diagnosticProfiles, reconcile] = await Promise.all([
      callTool('mission_infra_query', { action: 'list' }).catch(() => []),
      callTool('mission_infra_query', { action: 'health' }).catch((error) => ({ error: String(error) })),
      callTool('mission_infra_query', { action: 'skill_evidence', limit: 80 }).catch(() => ({ items: [] })),
      callTool('mission_infra_query', { action: 'credential_refs' }).catch(() => ({ credentialRefs: [] })),
      callTool('mission_infra_query', { action: 'diagnostic_profiles' }).catch(() => ({ profiles: [] })),
      callTool('mission_infra_query', { action: 'reconcile' }).catch((error) => ({ error: String(error) })),
    ]);
    return NextResponse.json({
      targets: Array.isArray(targets) ? targets : [],
      health,
      skillEvidence,
      credentialRefs,
      diagnosticProfiles,
      reconcile,
    });
  } catch (err) {
    return NextResponse.json({ error: String(err) }, { status: 502 });
  }
}
