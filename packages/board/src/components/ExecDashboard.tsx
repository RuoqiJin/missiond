'use client';

import { AutopilotMonitor } from './AutopilotMonitor';
import { DecisionDashboard } from './DecisionDashboard';

interface SlotDef {
  id: string;
  label: string;
  role: string;
  running?: boolean;
}

export function ExecDashboard({ slots }: { slots: SlotDef[] }) {
  return (
    <div className="flex-1 flex min-h-0 overflow-hidden">
      {/* Left: Autopilot Monitor (60%) */}
      <div className="w-[60%] min-w-0 overflow-hidden flex flex-col">
        <AutopilotMonitor slots={slots} />
      </div>

      {/* Divider */}
      <div className="w-px bg-neutral-800 shrink-0" />

      {/* Right: Decision Dashboard (40%) */}
      <div className="w-[40%] min-w-0 overflow-hidden flex flex-col">
        <DecisionDashboard />
      </div>
    </div>
  );
}
