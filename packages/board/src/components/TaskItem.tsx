import { useState } from 'react';
import { useSortable } from '@dnd-kit/sortable';
import { CSS } from '@dnd-kit/utilities';
import { GripVertical, Calendar, Check, Minus, ChevronRight, ChevronDown, Plus, Lock } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { cn } from '@/lib/utils';
import { CATEGORY_CONFIG, PRIORITY_CONFIG } from '../constants';
import type { Task } from '../types';

interface TaskItemProps {
  task: Task;
  depth?: number;
  childCount?: number;
  doneChildCount?: number;
  isExpanded?: boolean;
  onToggle: () => void;
  onClick: () => void;
  onExpand?: () => void;
  onAddSub?: (title: string) => void;
  children?: React.ReactNode;
}

export function TaskItem({
  task,
  depth = 0,
  childCount = 0,
  doneChildCount = 0,
  isExpanded = false,
  onToggle,
  onClick,
  onExpand,
  onAddSub,
  children,
}: TaskItemProps) {
  const [subInput, setSubInput] = useState('');
  const isDone = task.status === 'done';
  const isSkipped = task.status === 'skipped';
  const isRunning = task.status === 'running';
  const isVerifying = task.status === 'verifying';
  const isBlocked = task.status === 'blocked';
  const isFailed = task.status === 'failed';
  const isInactive = isDone || isSkipped;

  const {
    attributes,
    listeners,
    setNodeRef,
    setActivatorNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({
    id: task.id,
    data: { type: 'task', task },
    disabled: isInactive || depth > 0,
  });

  const style = {
    transform: CSS.Transform.toString(transform),
    transition,
  };
  const categoryConf = CATEGORY_CONFIG[task.category];
  const priorityConf = PRIORITY_CONFIG[task.priority];
  const isOverdue = task.dueDate && !isDone && new Date(task.dueDate) < new Date();
  const hasChildren = childCount > 0;
  const allChildrenDone = hasChildren && doneChildCount === childCount;

  const handleSubSubmit = () => {
    const trimmed = subInput.trim();
    if (!trimmed || !onAddSub) return;
    onAddSub(trimmed);
    setSubInput('');
  };

  return (
    <div>
      <div
        ref={setNodeRef}
        style={{ ...style, paddingLeft: `${12 + depth * 24}px`, paddingRight: '12px' }}
        className={cn(
          'flex items-start gap-2 py-2 rounded-lg border border-transparent transition-all group',
          'hover:bg-neutral-900/80 hover:border-neutral-800',
          isDragging && 'opacity-40',
          isInactive && 'opacity-50',
        )}
      >
        {/* Drag handle */}
        {!isInactive && depth === 0 ? (
          <button
            ref={setActivatorNodeRef}
            className="mt-1 opacity-0 group-hover:opacity-100 transition-opacity cursor-grab active:cursor-grabbing text-neutral-700 hover:text-neutral-500 shrink-0"
            {...attributes}
            {...listeners}
          >
            <GripVertical className="w-3.5 h-3.5" />
          </button>
        ) : (
          <div className="w-3.5 shrink-0" />
        )}

        {/* Expand/collapse */}
        {!isInactive ? (
          <button
            onClick={(e) => { e.stopPropagation(); onExpand?.(); }}
            className={cn(
              'mt-0.5 transition-colors shrink-0',
              hasChildren
                ? 'text-neutral-600 hover:text-neutral-400'
                : 'text-neutral-800 hover:text-neutral-500 opacity-0 group-hover:opacity-100',
            )}
          >
            {isExpanded
              ? <ChevronDown className="w-3.5 h-3.5" />
              : hasChildren
                ? <ChevronRight className="w-3.5 h-3.5" />
                : <Plus className="w-3.5 h-3.5" />
            }
          </button>
        ) : (
          <div className="w-3.5 shrink-0" />
        )}

        {/* Checkbox */}
        <button
          onClick={(e) => { e.stopPropagation(); onToggle(); }}
          className={cn(
            'mt-0.5 w-4 h-4 rounded border flex items-center justify-center shrink-0 transition-colors',
            isDone
              ? 'bg-green-500/20 border-green-500/40 text-green-400'
              : isSkipped
                ? 'bg-neutral-500/20 border-neutral-500/40 text-neutral-400'
                : isRunning
                  ? 'border-blue-500/40 animate-pulse'
                  : isFailed
                    ? 'bg-red-500/20 border-red-500/40 text-red-400'
                    : allChildrenDone
                      ? 'border-green-500/40 hover:border-green-500/60'
                      : 'border-neutral-700 hover:border-neutral-500',
          )}
        >
          {isDone && <Check className="w-3 h-3" />}
          {isSkipped && <Minus className="w-3 h-3" />}
        </button>

        {/* Content */}
        <div className="flex-1 min-w-0 cursor-pointer" onClick={onClick}>
          <div className="flex items-center gap-2">
            {!isInactive && <span className={cn('w-1.5 h-1.5 rounded-full shrink-0', priorityConf.dotColor)} />}
            <span className={cn(
              'text-sm truncate',
              isDone ? 'line-through text-neutral-600' : isSkipped ? 'text-neutral-500 italic' : isFailed ? 'text-red-400' : 'text-white',
            )}>
              {task.title}
            </span>
            {isRunning && <span className="text-[10px] px-1.5 py-0 h-4 rounded bg-blue-500/20 text-blue-400 border border-blue-500/30 shrink-0">Running</span>}
            {isVerifying && <span className="text-[10px] px-1.5 py-0 h-4 rounded bg-yellow-500/20 text-yellow-400 border border-yellow-500/30 shrink-0">Verifying</span>}
            {isBlocked && <span className="text-[10px] px-1.5 py-0 h-4 rounded bg-orange-500/20 text-orange-400 border border-orange-500/30 shrink-0">Blocked</span>}
            {isFailed && <span className="text-[10px] px-1.5 py-0 h-4 rounded bg-red-500/20 text-red-400 border border-red-500/30 shrink-0">Failed</span>}
            {task.claimExecutorId && (
              <span className="flex items-center gap-0.5 text-[10px] px-1.5 py-0 h-4 rounded bg-purple-500/20 text-purple-400 border border-purple-500/30 shrink-0" title={`Claimed by ${task.claimExecutorId} (${task.claimExecutorType || 'unknown'})`}>
                <Lock className="w-2.5 h-2.5" />
                {task.claimExecutorType === 'pty_slot' ? task.claimExecutorId : 'Session'}
              </span>
            )}

            {hasChildren && !isInactive && (
              <span className={cn(
                'text-[10px] shrink-0 tabular-nums',
                allChildrenDone ? 'text-green-500' : 'text-neutral-600',
              )}>
                [{doneChildCount}/{childCount}]
              </span>
            )}
          </div>

          {task.description && !isInactive && (
            <p className="text-xs text-neutral-600 line-clamp-1 mt-0.5 ml-3.5">{task.description}</p>
          )}

          {depth === 0 && (
            <div className="flex items-center gap-1.5 mt-1.5 ml-3.5 flex-wrap">
              <Badge variant="outline" className={cn('text-[10px] px-1.5 py-0 h-4 font-normal', categoryConf.className)}>
                {categoryConf.label}
              </Badge>
              {task.project && (
                <Badge variant="outline" className="text-[10px] px-1.5 py-0 h-4 font-normal bg-neutral-800/50 text-neutral-500 border-neutral-700/50">
                  {task.project}
                </Badge>
              )}
              {task.server && (
                <Badge variant="outline" className="text-[10px] px-1.5 py-0 h-4 font-normal bg-neutral-800/50 text-neutral-500 border-neutral-700/50">
                  {task.server}
                </Badge>
              )}
              {task.dueDate && (
                <span className={cn('flex items-center gap-0.5 text-[10px]', isOverdue ? 'text-red-400' : 'text-neutral-600')}>
                  <Calendar className="w-2.5 h-2.5" />
                  {task.dueDate}
                </span>
              )}
            </div>
          )}
        </div>
      </div>

      {/* Expanded children + inline add */}
      {isExpanded && !isInactive && (
        <div>
          {children}

          {onAddSub && (
            <div
              className="flex items-center gap-2 py-1.5 transition-opacity"
              style={{ paddingLeft: `${36 + (depth + 1) * 24}px`, paddingRight: '12px' }}
            >
              <Plus className="w-3 h-3 text-neutral-700 shrink-0" />
              <input
                value={subInput}
                onChange={(e) => setSubInput(e.target.value)}
                onKeyDown={(e) => e.key === 'Enter' && handleSubSubmit()}
                placeholder="添加子任务..."
                className="flex-1 bg-transparent text-xs text-neutral-400 placeholder:text-neutral-700 outline-none"
              />
            </div>
          )}
        </div>
      )}
    </div>
  );
}

export function TaskItemOverlay({ task }: { task: Task }) {
  const priorityConf = PRIORITY_CONFIG[task.priority];
  const categoryConf = CATEGORY_CONFIG[task.category];

  return (
    <div className="bg-neutral-900 border border-blue-500/40 rounded-lg px-3 py-2.5 shadow-xl shadow-blue-500/10 w-[400px]">
      <div className="flex items-center gap-2 ml-7">
        <span className={cn('w-1.5 h-1.5 rounded-full', priorityConf.dotColor)} />
        <span className="text-sm text-white truncate">{task.title}</span>
        <Badge variant="outline" className={cn('text-[10px] px-1.5 py-0 h-4 font-normal ml-1', categoryConf.className)}>
          {categoryConf.label}
        </Badge>
      </div>
    </div>
  );
}
