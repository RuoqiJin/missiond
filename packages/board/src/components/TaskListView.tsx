import { useState, useCallback, useMemo } from 'react';
import {
  DndContext,
  DragOverlay,
  closestCenter,
  PointerSensor,
  TouchSensor,
  KeyboardSensor,
  useSensor,
  useSensors,
  type DragStartEvent,
  type DragEndEvent,
} from '@dnd-kit/core';
import { SortableContext, verticalListSortingStrategy, sortableKeyboardCoordinates } from '@dnd-kit/sortable';
import { ChevronDown, ChevronRight, CheckCircle2, Trash2, EyeOff, SkipForward } from 'lucide-react';
import { cn } from '@/lib/utils';
import { useTaskCenterStore } from '../store';
import { CATEGORY_CONFIG, PRIORITY_CONFIG } from '../constants';
import { TaskItem, TaskItemOverlay } from './TaskItem';
import type { Task, GroupBy } from '../types';

function buildTree(tasks: Task[]): { roots: Task[]; childrenMap: Map<string, Task[]>; doneCountMap: Map<string, number> } {
  const childrenMap = new Map<string, Task[]>();
  const roots: Task[] = [];

  for (const task of tasks) {
    if (task.parentId) {
      const siblings = childrenMap.get(task.parentId) || [];
      siblings.push(task);
      childrenMap.set(task.parentId, siblings);
    } else {
      roots.push(task);
    }
  }

  for (const [, children] of childrenMap) {
    children.sort((a, b) => a.order - b.order);
  }

  const doneCountMap = new Map<string, number>();
  for (const [parentId, children] of childrenMap) {
    doneCountMap.set(parentId, children.filter((c) => c.status === 'done').length);
  }

  return { roots, childrenMap, doneCountMap };
}

interface TaskGroup {
  key: string;
  label: string;
  dotColor?: string;
  tasks: Task[];
}

function groupTasks(tasks: Task[], groupBy: GroupBy): TaskGroup[] {
  if (groupBy === 'none') {
    return [{ key: 'all', label: '', tasks }];
  }

  const map = new Map<string, Task[]>();
  const order: string[] = [];

  for (const task of tasks) {
    let key: string;
    if (groupBy === 'category') key = task.category;
    else if (groupBy === 'priority') key = task.priority;
    else key = task.project || '未关联项目';

    if (!map.has(key)) {
      map.set(key, []);
      order.push(key);
    }
    map.get(key)!.push(task);
  }

  if (groupBy === 'priority') {
    const priorityOrder = ['high', 'medium', 'low'];
    order.sort((a, b) => priorityOrder.indexOf(a) - priorityOrder.indexOf(b));
  } else if (groupBy === 'category') {
    const categoryOrder = ['deploy', 'dev', 'infra', 'test', 'other'];
    order.sort((a, b) => categoryOrder.indexOf(a) - categoryOrder.indexOf(b));
  }

  return order.map((key) => {
    let label = key;
    let dotColor: string | undefined;

    if (groupBy === 'category' && key in CATEGORY_CONFIG) {
      label = CATEGORY_CONFIG[key as keyof typeof CATEGORY_CONFIG].label;
    } else if (groupBy === 'priority' && key in PRIORITY_CONFIG) {
      const conf = PRIORITY_CONFIG[key as keyof typeof PRIORITY_CONFIG];
      label = conf.label + '优先级';
      dotColor = conf.dotColor;
    }

    return { key, label, dotColor, tasks: map.get(key)! };
  });
}

function TaskTree({
  tasks,
  depth,
  childrenMap,
  doneCountMap,
  expandedSet,
  onToggleExpand,
  onToggleTask,
  onClickTask,
  onAddSub,
}: {
  tasks: Task[];
  depth: number;
  childrenMap: Map<string, Task[]>;
  doneCountMap: Map<string, number>;
  expandedSet: Set<string>;
  onToggleExpand: (id: string) => void;
  onToggleTask: (id: string) => void;
  onClickTask: (task: Task) => void;
  onAddSub: (parentId: string, title: string) => void;
}) {
  return (
    <>
      {tasks.map((task) => {
        const children = childrenMap.get(task.id) || [];
        const openChildren = children.filter((c) => c.status === 'open');
        const doneChildren = children.filter((c) => c.status === 'done');
        const isExpanded = expandedSet.has(task.id);

        return (
          <TaskItem
            key={task.id}
            task={task}
            depth={depth}
            childCount={children.length}
            doneChildCount={doneCountMap.get(task.id) || 0}
            isExpanded={isExpanded}
            onToggle={() => onToggleTask(task.id)}
            onClick={() => onClickTask(task)}
            onExpand={() => onToggleExpand(task.id)}
            onAddSub={(title) => onAddSub(task.id, title)}
          >
            <TaskTree
              tasks={[...openChildren, ...doneChildren]}
              depth={depth + 1}
              childrenMap={childrenMap}
              doneCountMap={doneCountMap}
              expandedSet={expandedSet}
              onToggleExpand={onToggleExpand}
              onToggleTask={onToggleTask}
              onClickTask={onClickTask}
              onAddSub={onAddSub}
            />
          </TaskItem>
        );
      })}
    </>
  );
}

export function TaskListView() {
  const tasks = useTaskCenterStore((s) => s.tasks);
  const filters = useTaskCenterStore((s) => s.filters);
  const groupBy = useTaskCenterStore((s) => s.groupBy);
  const showDone = useTaskCenterStore((s) => s.showDone);
  const toggleTask = useTaskCenterStore((s) => s.toggleTask);
  const reorderTask = useTaskCenterStore((s) => s.reorderTask);
  const openEditDialog = useTaskCenterStore((s) => s.openEditDialog);
  const quickAddSub = useTaskCenterStore((s) => s.quickAddSub);
  const setShowDone = useTaskCenterStore((s) => s.setShowDone);
  const showHidden = useTaskCenterStore((s) => s.showHidden);
  const setShowHidden = useTaskCenterStore((s) => s.setShowHidden);
  const showSkipped = useTaskCenterStore((s) => s.showSkipped);
  const setShowSkipped = useTaskCenterStore((s) => s.setShowSkipped);
  const clearDoneTasks = useTaskCenterStore((s) => s.clearDoneTasks);

  const [activeTask, setActiveTask] = useState<Task | null>(null);
  const [expandedSet, setExpandedSet] = useState<Set<string>>(new Set());
  const [collapsedGroups, setCollapsedGroups] = useState<Set<string>>(new Set());

  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 8 } }),
    useSensor(TouchSensor, { activationConstraint: { delay: 200, tolerance: 5 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  );

  const filtered = useMemo(() => {
    return tasks.filter((task) => {
      // Exclude hidden and skipped tasks from main view
      if (task.hidden) return false;
      if (task.status === 'skipped') return false;
      if (filters.search) {
        const q = filters.search.toLowerCase();
        if (!task.title.toLowerCase().includes(q) && !task.description.toLowerCase().includes(q)) return false;
      }
      if (filters.category !== 'all' && task.category !== filters.category) return false;
      if (filters.priority !== 'all' && task.priority !== filters.priority) return false;
      return true;
    });
  }, [tasks, filters]);

  const hiddenTasks = useMemo(() =>
    tasks.filter((t) => t.hidden && t.status === 'open'),
    [tasks],
  );

  const skippedTasks = useMemo(() =>
    tasks.filter((t) => t.status === 'skipped' && !t.parentId),
    [tasks],
  );

  const { roots, childrenMap, doneCountMap } = useMemo(() => buildTree(filtered), [filtered]);

  const ACTIVE_STATUSES = new Set(['open', 'running', 'verifying', 'blocked', 'failed']);

  const openRoots = useMemo(() =>
    roots.filter((t) => ACTIVE_STATUSES.has(t.status)).sort((a, b) => a.order - b.order),
    [roots],
  );

  const doneRoots = useMemo(() =>
    roots.filter((t) => t.status === 'done'),
    [roots],
  );

  const groups = useMemo(() => groupTasks(openRoots, groupBy), [openRoots, groupBy]);
  const allRootIds = useMemo(() => openRoots.map((t) => t.id), [openRoots]);

  const toggleExpand = useCallback((id: string) => {
    setExpandedSet((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  const toggleGroup = useCallback((key: string) => {
    setCollapsedGroups((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  }, []);

  const handleDragStart = useCallback((event: DragStartEvent) => {
    const task = tasks.find((t) => t.id === event.active.id);
    if (task) setActiveTask(task);
  }, [tasks]);

  const handleDragEnd = useCallback((event: DragEndEvent) => {
    setActiveTask(null);
    const { active, over } = event;
    if (!over || active.id === over.id) return;
    const overIndex = openRoots.findIndex((t) => t.id === over.id);
    if (overIndex >= 0) reorderTask(String(active.id), overIndex);
  }, [openRoots, reorderTask]);

  const totalDoneCount = useMemo(() => tasks.filter((t) => t.status === 'done' && !t.parentId).length, [tasks]);

  return (
    <DndContext
      sensors={sensors}
      collisionDetection={closestCenter}
      onDragStart={handleDragStart}
      onDragEnd={handleDragEnd}
    >
      <SortableContext items={allRootIds} strategy={verticalListSortingStrategy}>
        <div className="space-y-0.5">
          {groups.map((group) => (
            <div key={group.key}>
              {group.label && (
                <button
                  onClick={() => toggleGroup(group.key)}
                  className="flex items-center gap-2 px-3 py-2 text-xs text-neutral-500 hover:text-neutral-400 transition-colors w-full"
                >
                  {collapsedGroups.has(group.key)
                    ? <ChevronRight className="w-3 h-3" />
                    : <ChevronDown className="w-3 h-3" />
                  }
                  {group.dotColor && <span className={cn('w-2 h-2 rounded-full', group.dotColor)} />}
                  <span className="font-medium">{group.label}</span>
                  <span className="text-neutral-700">{group.tasks.length}</span>
                </button>
              )}

              {!collapsedGroups.has(group.key) && (
                <TaskTree
                  tasks={group.tasks}
                  depth={0}
                  childrenMap={childrenMap}
                  doneCountMap={doneCountMap}
                  expandedSet={expandedSet}
                  onToggleExpand={toggleExpand}
                  onToggleTask={toggleTask}
                  onClickTask={openEditDialog}
                  onAddSub={quickAddSub}
                />
              )}
            </div>
          ))}

          {openRoots.length === 0 && (
            <div className="flex items-center justify-center py-16 text-sm text-neutral-700">
              没有待办任务
            </div>
          )}
        </div>
      </SortableContext>

      {totalDoneCount > 0 && (
        <div className="mt-6 border-t border-neutral-800/50 pt-4">
          <div className="flex items-center justify-between px-3">
            <button
              onClick={() => setShowDone(!showDone)}
              className="flex items-center gap-2 text-xs text-neutral-600 hover:text-neutral-500 transition-colors"
            >
              <CheckCircle2 className="w-3.5 h-3.5" />
              <span>已完成 ({totalDoneCount})</span>
              {showDone ? <ChevronDown className="w-3 h-3" /> : <ChevronRight className="w-3 h-3" />}
            </button>
            {showDone && (
              <button
                onClick={clearDoneTasks}
                className="flex items-center gap-1 text-[10px] text-neutral-700 hover:text-red-400 transition-colors"
              >
                <Trash2 className="w-3 h-3" />
                清除
              </button>
            )}
          </div>

          {showDone && (
            <div className="mt-2 space-y-0.5">
              <TaskTree
                tasks={doneRoots}
                depth={0}
                childrenMap={childrenMap}
                doneCountMap={doneCountMap}
                expandedSet={expandedSet}
                onToggleExpand={toggleExpand}
                onToggleTask={toggleTask}
                onClickTask={openEditDialog}
                onAddSub={quickAddSub}
              />
            </div>
          )}
        </div>
      )}

      {skippedTasks.length > 0 && (
        <div className="mt-6 border-t border-neutral-800/50 pt-4">
          <button
            onClick={() => setShowSkipped(!showSkipped)}
            className="flex items-center gap-2 px-3 text-xs text-neutral-600 hover:text-neutral-500 transition-colors"
          >
            <SkipForward className="w-3.5 h-3.5" />
            <span>已跳过 ({skippedTasks.length})</span>
            {showSkipped ? <ChevronDown className="w-3 h-3" /> : <ChevronRight className="w-3 h-3" />}
          </button>

          {showSkipped && (
            <div className="mt-2 space-y-0.5">
              {skippedTasks.sort((a, b) => a.order - b.order).map((task) => (
                <TaskItem
                  key={task.id}
                  task={task}
                  depth={0}
                  childCount={0}
                  doneChildCount={0}
                  isExpanded={false}
                  onToggle={() => toggleTask(task.id)}
                  onClick={() => openEditDialog(task)}
                  onExpand={() => {}}
                  onAddSub={() => {}}
                />
              ))}
            </div>
          )}
        </div>
      )}

      {hiddenTasks.length > 0 && (
        <div className="mt-6 border-t border-neutral-800/50 pt-4">
          <button
            onClick={() => setShowHidden(!showHidden)}
            className="flex items-center gap-2 px-3 text-xs text-neutral-600 hover:text-neutral-500 transition-colors"
          >
            <EyeOff className="w-3.5 h-3.5" />
            <span>手动处理 ({hiddenTasks.length})</span>
            {showHidden ? <ChevronDown className="w-3 h-3" /> : <ChevronRight className="w-3 h-3" />}
          </button>

          {showHidden && (
            <div className="mt-2 space-y-0.5">
              {hiddenTasks.sort((a, b) => a.order - b.order).map((task) => (
                <TaskItem
                  key={task.id}
                  task={task}
                  depth={0}
                  childCount={0}
                  doneChildCount={0}
                  isExpanded={false}
                  onToggle={() => toggleTask(task.id)}
                  onClick={() => openEditDialog(task)}
                  onExpand={() => {}}
                  onAddSub={() => {}}
                />
              ))}
            </div>
          )}
        </div>
      )}

      <DragOverlay>
        {activeTask ? <TaskItemOverlay task={activeTask} /> : null}
      </DragOverlay>
    </DndContext>
  );
}
