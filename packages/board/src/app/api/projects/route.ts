import { NextResponse } from 'next/server';
import { callTool } from '@/lib/missiond';

type ProjectRow = Record<string, unknown> & { id?: string };
type UniverseResponse = { services?: Array<Record<string, unknown>> };
type DeploymentChannelsResponse = {
  channels?: Array<Record<string, unknown>>;
  services?: Array<Record<string, unknown>>;
  diagnostics?: unknown[];
  summary?: unknown;
};
type DeploymentChannel = Record<string, unknown> & {
  id: string;
  serviceId: string;
  surface: string;
  substrate?: string;
};

export async function GET() {
  try {
    const listResult = await callTool('mission_project', { action: 'list' });
    const projects: ProjectRow[] = Array.isArray(listResult) ? listResult.filter(isProjectRow) : [];
    const universeResult = await callTool('mission_project', { action: 'universe' }).catch(() => null);
    const services = Array.isArray((universeResult as UniverseResponse | null)?.services)
      ? ((universeResult as UniverseResponse).services ?? [])
      : [];
    const deploymentResult = await callTool('mission_project', { action: 'deployment_channels' }).catch(() => null);
    const deploymentChannels = Array.isArray((deploymentResult as DeploymentChannelsResponse | null)?.channels)
      ? ((deploymentResult as DeploymentChannelsResponse).channels ?? []).filter(isRecord).map(normalizeFlatChannel)
      : [];
    const deploymentServices = Array.isArray((deploymentResult as DeploymentChannelsResponse | null)?.services)
      ? ((deploymentResult as DeploymentChannelsResponse).services ?? []).filter(isRecord)
      : [];
    const byProject = new Map<string, Array<Record<string, unknown>>>();
    for (const service of services) {
      const project = typeof service.project === 'string' ? service.project : undefined;
      if (!project) continue;
      byProject.set(project, [...(byProject.get(project) || []), service]);
    }
    const channelsByProject = new Map<string, DeploymentChannel[]>();
    for (const channel of deploymentChannels) {
      const projectId = channel.projectId || channel.project_id || channel.serviceId;
      if (typeof projectId !== 'string' || !projectId) continue;
      channelsByProject.set(projectId, [...(channelsByProject.get(projectId) || []), channel]);
    }
    const byId = new Map<string, ProjectRow>();
    for (const project of projects) {
      if (typeof project.id === 'string' && project.id) byId.set(project.id, project);
    }
    for (const service of [...services, ...deploymentServices]) {
      const projectId = stringValue(service.project ?? service.project_id ?? service.projectId ?? service.id);
      if (!projectId || byId.has(projectId)) continue;
      byId.set(projectId, {
        id: projectId,
        path: stringValue(service.root) || '',
        kind: 'compiled',
        active: true,
        lispFiles: [],
        lispCount: 0,
      });
    }
    const enriched = [...byId.values()].map((project) => {
      const id = typeof project.id === 'string' ? project.id : '';
      const runtimeServices = byProject.get(id) || [];
      const explicitChannels = channelsByProject.get(id) || [];
      return {
        ...project,
        runtimeServices,
        deploymentChannels: explicitChannels.length
          ? explicitChannels
          : runtimeServices.flatMap(normalizeDeploymentChannels),
        deploymentChannelSummary: (deploymentResult as DeploymentChannelsResponse | null)?.summary,
        deploymentChannelDiagnostics: (deploymentResult as DeploymentChannelsResponse | null)?.diagnostics ?? [],
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

function normalizeDeploymentChannels(service: Record<string, unknown>): DeploymentChannel[] {
  const explicit = arrayValue(service.deploymentChannels ?? service.deployment_channels)
    .filter(isRecord)
    .map((channel, index) => normalizeChannel(channel, service, index))
    .filter((channel): channel is DeploymentChannel => !!channel);
  if (explicit.length > 0) return explicit;

  const channels: DeploymentChannel[] = [];
  const serviceId = stringValue(service.id) || 'service';
  const projectId = stringValue(service.project) || serviceId;
  const buildLane = recordValue(service.buildLane ?? service.build_lane);
  if (buildLane) {
    channels.push(compactChannel({
      id: `${serviceId}:build`,
      serviceId,
      projectId,
      surface: 'build',
      substrate: stringValue(buildLane.id) || 'privatecloud',
      buildLane: stringValue(buildLane.id),
      builder: stringValue(buildLane.builder),
      executor: stringValue(buildLane.executor),
      sourceSync: stringValue(buildLane.sourceSync ?? buildLane.source_sync),
      artifactLane: stringValue(buildLane.artifactLane ?? buildLane.artifact_lane),
      image: stringValue(buildLane.image),
      authority: stringValue(buildLane.authority) || 'deploy-center',
    }));
  }

  const deployment = recordValue(service.deployment);
  if (deployment) {
    channels.push(compactChannel({
      id: `${serviceId}:runtime`,
      serviceId,
      projectId,
      surface: 'runtime',
      substrate: stringValue(deployment.substrate),
      deployCenterSlug: stringValue(deployment.dcSlug ?? deployment.dc_slug),
      runtimeTarget: stringValue(deployment.runtimeTarget ?? deployment.runtime_target),
      executor: stringValue(deployment.executor),
      container: stringValue(deployment.container),
      hostBind: stringValue(deployment.hostBind ?? deployment.host_bind ?? deployment.localBind ?? deployment.local_bind),
      proxy: stringValue(deployment.proxy),
      imageEnv: stringValue(deployment.imageEnv ?? deployment.image_env),
      artifactLane: stringValue(deployment.artifactLane ?? deployment.artifact_lane ?? deployment.artifact_delivery_lane),
      authority: stringValue(deployment.authority),
    }));
  }

  const frontendDeployment = recordValue(service.frontendDeployment ?? service.frontend_deployment);
  if (frontendDeployment) {
    channels.push(compactChannel({
      id: `${serviceId}:frontend`,
      serviceId,
      projectId,
      surface: 'frontend',
      substrate: stringValue(frontendDeployment.substrate),
      project: stringValue(frontendDeployment.project),
      rootDirectory: stringValue(frontendDeployment.rootDirectory ?? frontendDeployment.root_directory),
      productionDomain: stringValue(frontendDeployment.productionDomain ?? frontendDeployment.production_domain),
      fallbackDomain: stringValue(frontendDeployment.fallbackDomain ?? frontendDeployment.fallback_domain),
      authority: stringValue(frontendDeployment.authority) || 'vercel',
    }));
  }

  return channels;
}

function normalizeFlatChannel(channel: Record<string, unknown>): DeploymentChannel {
  return compactChannel({
    ...channel,
    id: stringValue(channel.id) || `${stringValue(channel.service_id ?? channel.serviceId) || 'service'}:${stringValue(channel.surface) || 'runtime'}`,
    serviceId: stringValue(channel.serviceId ?? channel.service_id) || 'service',
    projectId: stringValue(channel.projectId ?? channel.project_id),
    surface: stringValue(channel.surface) || 'runtime',
    substrate: stringValue(channel.substrate),
    channelKind: stringValue(channel.channelKind ?? channel.channel_kind),
    deployCenterSlug: stringValue(channel.deployCenterSlug ?? channel.deploy_center_slug),
    runtimeTarget: stringValue(channel.runtimeTarget ?? channel.runtime_target),
    artifactLane: stringValue(channel.artifactLane ?? channel.artifact_lane),
    buildLane: stringValue(channel.buildLane ?? channel.build_lane),
    sourceSync: stringValue(channel.sourceSync ?? channel.source_sync),
    workflow: stringValue(channel.workflow),
    sourceRef: stringValue(channel.sourceRef ?? channel.source_ref),
    declaredStatus: stringValue(channel.declaredStatus ?? channel.declared_status),
    observedStatus: stringValue(channel.observedStatus ?? channel.observed_status),
    driftStatus: stringValue(channel.driftStatus ?? channel.drift_status),
    targetSideBuildProhibited: channel.targetSideBuildProhibited ?? channel.target_side_build_prohibited,
  });
}

function normalizeChannel(
  channel: Record<string, unknown>,
  service: Record<string, unknown>,
  index: number,
): DeploymentChannel | null {
  const serviceId = stringValue(channel.serviceId ?? channel.service_id ?? service.id) || 'service';
  const surface = stringValue(channel.surface) || 'runtime';
  return compactChannel({
    ...channel,
    id: stringValue(channel.id) || `${serviceId}:${surface}:${index}`,
    serviceId,
    projectId: stringValue(channel.projectId ?? channel.project_id ?? service.project) || serviceId,
    surface,
    substrate: stringValue(channel.substrate),
    deployCenterSlug: stringValue(channel.deployCenterSlug ?? channel.deploy_center_slug),
    runtimeTarget: stringValue(channel.runtimeTarget ?? channel.runtime_target),
    artifactLane: stringValue(channel.artifactLane ?? channel.artifact_lane),
    buildLane: stringValue(channel.buildLane ?? channel.build_lane),
    sourceSync: stringValue(channel.sourceSync ?? channel.source_sync),
    workflow: stringValue(channel.workflow),
    sourceRef: stringValue(channel.sourceRef ?? channel.source_ref),
    channelKind: stringValue(channel.channelKind ?? channel.channel_kind),
    declaredStatus: stringValue(channel.declaredStatus ?? channel.declared_status),
    observedStatus: stringValue(channel.observedStatus ?? channel.observed_status),
    driftStatus: stringValue(channel.driftStatus ?? channel.drift_status),
    hostBind: stringValue(channel.hostBind ?? channel.host_bind),
    rootDirectory: stringValue(channel.rootDirectory ?? channel.root_directory),
    productionDomain: stringValue(channel.productionDomain ?? channel.production_domain),
    fallbackDomain: stringValue(channel.fallbackDomain ?? channel.fallback_domain),
    imageEnv: stringValue(channel.imageEnv ?? channel.image_env),
  });
}

function compactChannel(value: Record<string, unknown>): DeploymentChannel {
  return Object.fromEntries(
    Object.entries(value).filter(([, entry]) => entry !== undefined && entry !== null && entry !== ''),
  ) as DeploymentChannel;
}

function stringValue(value: unknown): string | undefined {
  return typeof value === 'string' && value.trim() ? value : undefined;
}

function recordValue(value: unknown): Record<string, unknown> | undefined {
  return isRecord(value) ? value : undefined;
}

function arrayValue(value: unknown): unknown[] {
  return Array.isArray(value) ? value : [];
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === 'object' && !Array.isArray(value);
}
