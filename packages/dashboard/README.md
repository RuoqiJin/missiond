# CC Tasks Dashboard

Web dashboard for monitoring Claude Code tasks across all sessions.

## Usage

```bash
# Start the dashboard
cd packages/dashboard
npx serve .

# Or with specific port
npx serve -p 3120 .
```

Then open http://localhost:3000 (or http://localhost:3120).

## Requirements

- missiond daemon running on port 9120
- WebSocket endpoint: `ws://localhost:9120/tasks`

## Features

- Real-time task updates via WebSocket
- Session overview with active indicators
- Task counts by status (pending, in-progress, completed)
- Auto-reconnect on disconnect

## Screenshot

```
┌────────────────────────────────────────────────────────┐
│  CC Tasks Dashboard                        ● Connected │
├────────────────────────────────────────────────────────┤
│  ┌──────────┐ ┌──────────┐ ┌──────────┐               │
│  │    2     │ │    5     │ │   12     │               │
│  │ Active   │ │ Pending  │ │ Done     │               │
│  └──────────┘ └──────────┘ └──────────┘               │
├────────────────────────────────────────────────────────┤
│  ● missiond                   2 active │ 3 pending    │
│    ~/Projects/missiond                                │
│    ┌─────────────────────────────────────────────────┐│
│    │ ◐ Build CC Tasks Web Dashboard                  ││
│    │ ○ Implement cross-machine Tasks sync            ││
│    └─────────────────────────────────────────────────┘│
└────────────────────────────────────────────────────────┘
```
