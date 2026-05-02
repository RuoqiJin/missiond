#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import {
  head,
  isList,
  nodeBool,
  nodeText,
  parseLisp,
  readKeywordProps,
} from './lib/missiond_lisp.mjs';

const BLUEPRINT = '.missiond/frontend/board-blueprint.lisp';
const OUTPUT = 'packages/board/src/generated/board-frontend-config.ts';

function main() {
  const opts = parseArgs(process.argv.slice(2));
  const repo = opts.repo;
  const blueprintRel = opts.blueprint;
  const outputRel = opts.output;
  const blueprintPath = path.join(repo, blueprintRel);
  const outputPath = path.join(repo, outputRel);
  const parsed = readConfig(blueprintPath, blueprintRel);
  const generated = renderConfig(parsed, { blueprintRel, outputRel });

  if (opts.json) {
    console.log(JSON.stringify({
      ok: true,
      blueprint: blueprintRel,
      output: outputRel,
      tabs: parsed.tabs.items.length,
      categories: parsed.taxonomy.categories.length,
      routes: parsed.eventRoutes.routes.reduce((sum, route) => sum + route.events.length, 0),
    }, null, 2));
  }

  if (opts.write) {
    fs.mkdirSync(path.dirname(outputPath), { recursive: true });
    fs.writeFileSync(outputPath, generated);
  }

  if (opts.check) {
    let current = '';
    try {
      current = fs.readFileSync(outputPath, 'utf8');
    } catch (err) {
      fail(`${outputRel}: cannot read generated config: ${err.message}`);
    }
    if (normalizeNewlines(current) !== normalizeNewlines(generated)) {
      fail(`${outputRel}: generated config is stale; run node scripts/project-frontend-board-config.mjs --write`);
    }
  }

  if (!opts.write && !opts.check && !opts.json) {
    process.stdout.write(generated);
  }
}

function parseArgs(argv) {
  const opts = {
    repo: process.cwd(),
    blueprint: BLUEPRINT,
    output: OUTPUT,
    write: false,
    check: false,
    json: false,
  };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--write') opts.write = true;
    else if (arg === '--check') opts.check = true;
    else if (arg === '--json') opts.json = true;
    else if (arg === '--repo') opts.repo = argv[++i] ?? fail('--repo requires a value');
    else if (arg.startsWith('--repo=')) opts.repo = arg.slice('--repo='.length);
    else if (arg === '--blueprint') opts.blueprint = argv[++i] ?? fail('--blueprint requires a value');
    else if (arg.startsWith('--blueprint=')) opts.blueprint = arg.slice('--blueprint='.length);
    else if (arg === '--output') opts.output = argv[++i] ?? fail('--output requires a value');
    else if (arg.startsWith('--output=')) opts.output = arg.slice('--output='.length);
    else if (arg === '-h' || arg === '--help') {
      console.log('Usage: node scripts/project-frontend-board-config.mjs [--write|--check|--json] [--repo <path>] [--blueprint <path>] [--output <path>]');
      process.exit(0);
    } else {
      fail(`unknown arg: ${arg}`);
    }
  }
  return opts;
}

function readConfig(filePath, fileLabel) {
  let source;
  try {
    source = fs.readFileSync(filePath, 'utf8');
  } catch (err) {
    fail(`${fileLabel}: cannot read: ${err.message}`);
  }

  let forms;
  try {
    forms = parseLisp(source, fileLabel);
  } catch (err) {
    fail(`${fileLabel}:${err.line ?? 1}:${err.column ?? 1}: ${err.message}`);
  }

  const root = forms.find((form) => isList(form) && head(form) === 'missiond-frontend-blueprint');
  if (!root) fail(`${fileLabel}: missing missiond-frontend-blueprint root`);
  const runtime = child(root, 'frontend-runtime-config');
  if (!runtime) fail(`${fileLabel}: missing frontend-runtime-config`);

  const props = readKeywordProps(runtime, { start: 1 });
  const tabsNode = child(runtime, 'tabs');
  const taxonomyNode = child(runtime, 'task-taxonomy');
  const flowNode = child(runtime, 'flow');
  const eventRoutesNode = child(runtime, 'event-routes');
  const timelineVisualsNode = child(runtime, 'timeline-visuals');
  for (const [name, node] of [['tabs', tabsNode], ['task-taxonomy', taxonomyNode], ['flow', flowNode], ['event-routes', eventRoutesNode], ['timeline-visuals', timelineVisualsNode]]) {
    if (!node) fail(`${fileLabel}: frontend-runtime-config missing (${name} ...)`);
  }

  return {
    schema: textProp(props, ':schema'),
    generator: textProp(props, ':generator'),
    checker: textProp(props, ':checker'),
    output: textProp(props, ':output'),
    tabs: parseTabs(tabsNode),
    taxonomy: parseTaxonomy(taxonomyNode),
    flow: parseFlow(flowNode),
    eventRoutes: parseEventRoutes(eventRoutesNode),
    timelineVisuals: parseTimelineVisuals(timelineVisualsNode),
  };
}

function parseTabs(node) {
  const props = readKeywordProps(node, { start: 1 });
  const items = children(node, 'tab').map((tab) => {
    const p = readKeywordProps(tab, { start: 1 });
    return {
      id: requiredText(p, ':id', 'tab'),
      label: requiredText(p, ':label', 'tab'),
      icon: requiredText(p, ':icon', 'tab'),
    };
  });
  const migrations = children(node, 'migration').map((migration) => {
    const p = readKeywordProps(migration, { start: 1 });
    return {
      from: requiredText(p, ':from', 'migration'),
      to: requiredText(p, ':to', 'migration'),
    };
  });
  return {
    default: requiredText(props, ':default', 'tabs'),
    items,
    migrations,
  };
}

function parseTaxonomy(node) {
  return {
    categories: children(node, 'category').map((category) => {
      const p = readKeywordProps(category, { start: 1 });
      return {
        id: requiredText(p, ':id', 'category'),
        label: requiredText(p, ':label', 'category'),
        className: requiredText(p, ':className', 'category'),
      };
    }),
    priorities: children(node, 'priority').map((priority) => {
      const p = readKeywordProps(priority, { start: 1 });
      return {
        id: requiredText(p, ':id', 'priority'),
        label: requiredText(p, ':label', 'priority'),
        dotColor: requiredText(p, ':dotColor', 'priority'),
      };
    }),
    groupOptions: children(node, 'group-option').map((option) => {
      const p = readKeywordProps(option, { start: 1 });
      return {
        value: requiredText(p, ':value', 'group-option'),
        label: requiredText(p, ':label', 'group-option'),
      };
    }),
    serverOptions: children(node, 'server-option').map((option) => requiredText(readKeywordProps(option, { start: 1 }), ':value', 'server-option')),
  };
}

function parseFlow(node) {
  return {
    templates: children(node, 'template').map((template) => {
      const p = readKeywordProps(template, { start: 1 });
      const value = nodeText(p[':value']?.value);
      if (value == null) fail('template missing :value');
      return {
        value,
        label: requiredText(p, ':label', 'template'),
      };
    }),
    phases: children(node, 'phase').map((phase) => {
      const p = readKeywordProps(phase, { start: 1 });
      return {
        id: requiredText(p, ':id', 'phase'),
        label: requiredText(p, ':label', 'phase'),
      };
    }),
  };
}

function parseEventRoutes(node) {
  const props = readKeywordProps(node, { start: 1 });
  return {
    resyncBumps: listTexts(props[':resync-bumps']?.value),
    routes: children(node, 'route').map((route) => {
      const p = readKeywordProps(route, { start: 1 });
      return {
        events: requiredList(p, ':events', 'route'),
        bump: requiredList(p, ':bump', 'route'),
        delayMs: optionalNumber(p, ':delay-ms'),
        healthSnapshot: nodeBool(p[':health-snapshot']?.value) === true,
        deployCategoryBump: nodeBool(p[':deploy-category-bump']?.value) === true,
      };
    }),
    customEvents: children(node, 'custom-event').map((custom) => {
      const p = readKeywordProps(custom, { start: 1 });
      return {
        event: requiredText(p, ':event', 'custom-event'),
        name: requiredText(p, ':name', 'custom-event'),
        detail: requiredList(p, ':detail', 'custom-event'),
      };
    }),
    prefixRoutes: children(node, 'prefix-route').map((route) => {
      const p = readKeywordProps(route, { start: 1 });
      return {
        prefix: requiredText(p, ':prefix', 'prefix-route'),
        bump: requiredList(p, ':bump', 'prefix-route'),
        delayMs: optionalNumber(p, ':delay-ms'),
      };
    }),
  };
}

function parseTimelineVisuals(node) {
  return {
    events: children(node, 'event').map((event) => {
      const p = readKeywordProps(event, { start: 1 });
      return {
        type: requiredText(p, ':type', 'event'),
        dot: requiredText(p, ':dot', 'event'),
        glow: requiredText(p, ':glow', 'event'),
        bg: requiredText(p, ':bg', 'event'),
        text: requiredText(p, ':text', 'event'),
        label: requiredText(p, ':label', 'event'),
        icon: requiredText(p, ':icon', 'event'),
      };
    }),
    slotColors: children(node, 'slot-color').map((slot) => {
      const p = readKeywordProps(slot, { start: 1 });
      return {
        id: requiredText(p, ':id', 'slot-color'),
        badge: requiredText(p, ':badge', 'slot-color'),
        border: requiredText(p, ':border', 'slot-color'),
        line: requiredText(p, ':line', 'slot-color'),
      };
    }),
    slotFallbackLines: children(node, 'slot-fallback-line').map((line) => requiredText(readKeywordProps(line, { start: 1 }), ':value', 'slot-fallback-line')),
    swimlanes: children(node, 'swimlane').map((lane) => {
      const p = readKeywordProps(lane, { start: 1 });
      return {
        id: requiredText(p, ':id', 'swimlane'),
        label: requiredText(p, ':label', 'swimlane'),
        dot: requiredText(p, ':dot', 'swimlane'),
        css: requiredText(p, ':css', 'swimlane'),
        bg: requiredText(p, ':bg', 'swimlane'),
        types: requiredList(p, ':types', 'swimlane'),
      };
    }),
    sessionColors: children(node, 'session-color').map((color) => {
      const p = readKeywordProps(color, { start: 1 });
      return {
        dot: requiredText(p, ':dot', 'session-color'),
        line: requiredText(p, ':line', 'session-color'),
        ring: requiredText(p, ':ring', 'session-color'),
      };
    }),
    windowOptions: children(node, 'window-option').map((option) => {
      const p = readKeywordProps(option, { start: 1 });
      return {
        label: requiredText(p, ':label', 'window-option'),
        value: requiredText(p, ':value', 'window-option'),
      };
    }),
  };
}

function renderConfig(config, { blueprintRel, outputRel }) {
  const tabIds = config.tabs.items.map((tab) => tab.id);
  const eventKeys = config.eventRoutes.resyncBumps;
  const phases = config.flow.phases.map((phase) => phase.id);
  const lines = [
    '// GENERATED FILE - do not edit by hand.',
    `// GENERATED_FROM: ${blueprintRel}`,
    `// GENERATED_TO: ${outputRel}`,
    '// To refresh: node scripts/project-frontend-board-config.mjs --write',
    '',
    "import type { FlowPhase, GroupBy, TaskCategory, TaskPriority } from '../types';",
    '',
    `export type BoardTabId = ${union(tabIds)};`,
    `export type EventVersionKey = ${union(eventKeys)};`,
    '',
    'export interface BoardTabConfig {',
    '  id: BoardTabId;',
    '  label: string;',
    '  icon: string;',
    '}',
    '',
    'export interface EventRouteConfig {',
    '  events: readonly string[];',
    '  bump: readonly EventVersionKey[];',
    '  delayMs?: number;',
    '  healthSnapshot?: boolean;',
    '  deployCategoryBump?: boolean;',
    '}',
    '',
    'export interface EventCustomEventConfig {',
    '  event: string;',
    '  name: string;',
    '  detail: readonly string[];',
    '}',
    '',
    'export interface EventPrefixRouteConfig {',
    '  prefix: string;',
    '  bump: readonly EventVersionKey[];',
    '  delayMs?: number;',
    '}',
    '',
    'export interface TimelineEventVisualConfig {',
    '  dot: string;',
    '  glow: string;',
    '  bg: string;',
    '  text: string;',
    '  label: string;',
    '  icon: string;',
    '}',
    '',
    'export interface TimelineSlotColorConfig {',
    '  badge: string;',
    '  border: string;',
    '  line: string;',
    '}',
    '',
    'export interface TimelineSwimlaneConfig {',
    '  id: string;',
    '  label: string;',
    '  accent: { dot: string; css: string; bg: string };',
    '  types: readonly string[];',
    '}',
    '',
    `export const DEFAULT_TAB: BoardTabId = ${q(config.tabs.default)};`,
    `export const BOARD_TABS = ${arrayLiteral(config.tabs.items, renderTab)} as const satisfies readonly BoardTabConfig[];`,
    `export const TAB_MIGRATION = ${objectLiteral(config.tabs.migrations.map((m) => [m.from, m.to]), 0)} as const satisfies Record<string, BoardTabId>;`,
    '',
    `export const CATEGORY_CONFIG = ${objectLiteral(config.taxonomy.categories.map((c) => [c.id, { label: c.label, className: c.className }]), 0)} as const satisfies Record<TaskCategory, { label: string; className: string }>;`,
    `export const PRIORITY_CONFIG = ${objectLiteral(config.taxonomy.priorities.map((p) => [p.id, { label: p.label, dotColor: p.dotColor }]), 0)} as const satisfies Record<TaskPriority, { label: string; dotColor: string }>;`,
    `export const GROUP_OPTIONS = ${arrayLiteral(config.taxonomy.groupOptions, (item) => `{ value: ${q(item.value)} as GroupBy, label: ${q(item.label)} }`)} as const satisfies readonly { value: GroupBy; label: string }[];`,
    `export const SERVER_OPTIONS = ${arrayLiteral(config.taxonomy.serverOptions, (value) => q(value))} as const;`,
    '',
    `export const FLOW_TEMPLATE_OPTIONS = ${arrayLiteral(config.flow.templates, (item) => `{ value: ${q(item.value)}, label: ${q(item.label)} }`)} as const;`,
    `export const FLOW_PHASES = ${arrayLiteral(phases, (phase) => `${q(phase)} as FlowPhase`)} as const satisfies readonly FlowPhase[];`,
    `export const FLOW_PHASE_LABELS = ${objectLiteral(config.flow.phases.map((phase) => [phase.id, phase.label]), 0)} as const satisfies Record<FlowPhase, string>;`,
    '',
    `export const RESYNC_VERSION_KEYS = ${arrayLiteral(eventKeys, (key) => `${q(key)} as EventVersionKey`)} as const satisfies readonly EventVersionKey[];`,
    `export const EVENT_ROUTE_TABLE = ${arrayLiteral(config.eventRoutes.routes, renderRoute)} as const satisfies readonly EventRouteConfig[];`,
    `export const EVENT_CUSTOM_EVENTS = ${arrayLiteral(config.eventRoutes.customEvents, renderCustomEvent)} as const satisfies readonly EventCustomEventConfig[];`,
    `export const EVENT_PREFIX_ROUTES = ${arrayLiteral(config.eventRoutes.prefixRoutes, renderPrefixRoute)} as const satisfies readonly EventPrefixRouteConfig[];`,
    '',
    `export const TIMELINE_EVENT_VISUALS = ${objectLiteral(config.timelineVisuals.events.map((event) => [event.type, { dot: event.dot, glow: event.glow, bg: event.bg, text: event.text, label: event.label, icon: event.icon }]), 0)} as const satisfies Record<string, TimelineEventVisualConfig>;`,
    `export const TIMELINE_SLOT_COLORS = ${objectLiteral(config.timelineVisuals.slotColors.map((slot) => [slot.id, { badge: slot.badge, border: slot.border, line: slot.line }]), 0)} as const satisfies Record<string, TimelineSlotColorConfig>;`,
    `export const TIMELINE_SLOT_FALLBACK_LINES = ${arrayLiteral(config.timelineVisuals.slotFallbackLines, q)} as const;`,
    `export const TIMELINE_SWIMLANES = ${arrayLiteral(config.timelineVisuals.swimlanes, renderSwimlane)} as const satisfies readonly TimelineSwimlaneConfig[];`,
    `export const TIMELINE_SESSION_COLORS = ${arrayLiteral(config.timelineVisuals.sessionColors, renderSessionColor)} as const;`,
    `export const TIMELINE_WINDOW_OPTIONS = ${arrayLiteral(config.timelineVisuals.windowOptions, renderWindowOption)} as const;`,
    '',
  ];
  return lines.join('\n');
}

function renderTab(tab) {
  return `{ id: ${q(tab.id)}, label: ${q(tab.label)}, icon: ${q(tab.icon)} }`;
}

function renderRoute(route) {
  const parts = [
    `events: ${arrayLiteral(route.events, q)}`,
    `bump: ${arrayLiteral(route.bump, (key) => `${q(key)} as EventVersionKey`)}`,
  ];
  if (route.delayMs != null) parts.push(`delayMs: ${route.delayMs}`);
  if (route.healthSnapshot) parts.push('healthSnapshot: true');
  if (route.deployCategoryBump) parts.push('deployCategoryBump: true');
  return `{ ${parts.join(', ')} }`;
}

function renderCustomEvent(custom) {
  return `{ event: ${q(custom.event)}, name: ${q(custom.name)}, detail: ${arrayLiteral(custom.detail, q)} }`;
}

function renderPrefixRoute(route) {
  const parts = [
    `prefix: ${q(route.prefix)}`,
    `bump: ${arrayLiteral(route.bump, (key) => `${q(key)} as EventVersionKey`)}`,
  ];
  if (route.delayMs != null) parts.push(`delayMs: ${route.delayMs}`);
  return `{ ${parts.join(', ')} }`;
}

function renderSwimlane(lane) {
  return `{ id: ${q(lane.id)}, label: ${q(lane.label)}, accent: { dot: ${q(lane.dot)}, css: ${q(lane.css)}, bg: ${q(lane.bg)} }, types: ${arrayLiteral(lane.types, q)} }`;
}

function renderSessionColor(color) {
  return `{ dot: ${q(color.dot)}, line: ${q(color.line)}, ring: ${q(color.ring)} }`;
}

function renderWindowOption(option) {
  return `{ label: ${q(option.label)}, value: ${q(option.value)} }`;
}

function arrayLiteral(items, render) {
  if (items.length === 0) return '[]';
  return `[\n${items.map((item) => `  ${render(item)}`).join(',\n')},\n]`;
}

function objectLiteral(entries, indentLevel) {
  const indent = '  '.repeat(indentLevel);
  const inner = '  '.repeat(indentLevel + 1);
  if (entries.length === 0) return '{}';
  const body = entries.map(([key, value]) => {
    const rendered = typeof value === 'string' ? q(value) : inlineObject(value);
    return `${inner}${safeKey(key)}: ${rendered},`;
  }).join('\n');
  return `{\n${body}\n${indent}}`;
}

function inlineObject(value) {
  const parts = Object.entries(value).map(([key, val]) => `${key}: ${q(val)}`);
  return `{ ${parts.join(', ')} }`;
}

function safeKey(key) {
  return /^[A-Za-z_$][A-Za-z0-9_$]*$/.test(key) ? key : q(key);
}

function union(values) {
  return values.map(q).join(' | ');
}

function q(value) {
  return JSON.stringify(value);
}

function child(node, name) {
  return node.children.find((n) => isList(n) && head(n) === name);
}

function children(node, name) {
  return node.children.filter((n) => isList(n) && head(n) === name);
}

function textProp(props, key) {
  return nodeText(props[key]?.value);
}

function requiredText(props, key, label) {
  const text = textProp(props, key);
  if (text == null) fail(`${label} missing ${key}`);
  return text;
}

function requiredList(props, key, label) {
  const values = listTexts(props[key]?.value);
  if (values.length === 0) fail(`${label} missing non-empty ${key}`);
  return values;
}

function listTexts(node) {
  if (!node || !isList(node)) return [];
  return node.children.map((child) => nodeText(child)).filter((value) => value != null && value !== '');
}

function optionalNumber(props, key) {
  const raw = textProp(props, key);
  if (raw == null) return undefined;
  const parsed = Number.parseInt(raw, 10);
  if (!Number.isFinite(parsed)) fail(`expected numeric ${key}, got ${JSON.stringify(raw)}`);
  return parsed;
}

function normalizeNewlines(text) {
  return text.replace(/\r\n/g, '\n');
}

function fail(message) {
  console.error(message);
  process.exit(2);
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main();
}
