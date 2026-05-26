import { BOARD_TASK_ACTIONS } from '@/generated/board-frontend-config';
import { callTool } from '@/lib/missiond';

type BoardAction = (typeof BOARD_TASK_ACTIONS)[number]['name'];

const BOARD_TOOL_BY_ACTION = Object.fromEntries(
  BOARD_TASK_ACTIONS.map((action) => [action.name, action.tool]),
) as Record<BoardAction, string>;

export async function callBoardTool(
  action: BoardAction,
  args: Record<string, unknown> = {},
): Promise<unknown> {
  return callTool(BOARD_TOOL_BY_ACTION[action], args);
}

export const boardClient = {
  list(args: Record<string, unknown> = {}) {
    return callBoardTool('list', args);
  },
  get(id: string) {
    return callBoardTool('get', { id });
  },
  create(args: Record<string, unknown>) {
    return callBoardTool('create', args);
  },
  update(id: string, args: Record<string, unknown>) {
    return callBoardTool('update', { id, ...args });
  },
  delete(id: string) {
    return callBoardTool('delete', { id });
  },
  toggle(id: string) {
    return callBoardTool('toggle', { id });
  },
  addNote(
    id: string,
    args: { content: unknown; noteType?: unknown; author?: unknown },
  ) {
    return callBoardTool('note-add', {
      taskId: id,
      content: args.content,
      noteType: args.noteType,
      author: args.author,
    });
  },
} as const;
