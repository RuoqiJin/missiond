'use client';

import { useState, useEffect, useCallback } from 'react';
import { Plus, ChevronDown, ChevronRight, Send, CheckCircle2, Play, Pause, Pencil, Trash2 } from 'lucide-react';
import { cn } from '@/lib/utils';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Badge } from '@/components/ui/badge';
import { useTaskCenterStore } from '../store';
import { PRIORITY_CONFIG } from '../constants';
import * as api from '../api';
import type { Task, TaskNote, TaskPriority } from '../types';

const STATUS_CONFIG: Record<string, { label: string; color: string; bg: string }> = {
  open: { label: '新课题', color: 'text-cyan-400', bg: 'bg-cyan-500/10 border-cyan-500/20' },
  running: { label: '研究中', color: 'text-amber-400', bg: 'bg-amber-500/10 border-amber-500/20' },
  blocked: { label: '待输入', color: 'text-red-400', bg: 'bg-red-500/10 border-red-500/20' },
  done: { label: '已结论', color: 'text-green-400', bg: 'bg-green-500/10 border-green-500/20' },
  skipped: { label: '搁置', color: 'text-neutral-500', bg: 'bg-neutral-500/10 border-neutral-500/20' },
};

function timeAgo(dateStr: string): string {
  const diff = Date.now() - new Date(dateStr).getTime();
  const mins = Math.floor(diff / 60000);
  if (mins < 1) return '刚刚';
  if (mins < 60) return `${mins}分钟前`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}小时前`;
  const days = Math.floor(hours / 24);
  if (days < 30) return `${days}天前`;
  return new Date(dateStr).toLocaleDateString('zh-CN');
}

function formatDate(dateStr: string): string {
  return new Date(dateStr).toLocaleDateString('zh-CN', { month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit' });
}

function ResearchCard({ task, onRefresh }: { task: Task; onRefresh: () => void }) {
  const [expanded, setExpanded] = useState(() => task.status === 'open' || task.status === 'running');
  const [notes, setNotes] = useState<TaskNote[]>([]);
  const [notesLoaded, setNotesLoaded] = useState(false);
  const [newNote, setNewNote] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const openEditDialog = useTaskCenterStore((s) => s.openEditDialog);

  const loadNotes = useCallback(async () => {
    try {
      const data = await api.fetchTaskWithNotes(task.id);
      setNotes(data.notes || []);
      setNotesLoaded(true);
    } catch {
      setNotesLoaded(true);
    }
  }, [task.id]);

  useEffect(() => {
    if (expanded && !notesLoaded) loadNotes();
  }, [expanded, notesLoaded, loadNotes]);

  const addNote = async () => {
    if (!newNote.trim() || submitting) return;
    setSubmitting(true);
    try {
      const note = await api.addTaskNote(task.id, newNote.trim());
      setNotes((prev) => [...prev, note]);
      setNewNote('');
    } catch (e) {
      console.error('Failed to add note:', e);
    } finally {
      setSubmitting(false);
    }
  };

  const cycleStatus = async () => {
    const order = ['open', 'running', 'done'];
    const current = order.indexOf(task.status);
    const next = order[(current + 1) % order.length];
    try {
      await api.updateTask(task.id, { status: next });
      onRefresh();
    } catch (e) {
      console.error('Failed to update status:', e);
    }
  };

  const deleteTask = async () => {
    try {
      await api.deleteTask(task.id);
      onRefresh();
    } catch (e) {
      console.error('Failed to delete:', e);
    }
  };

  const statusConf = STATUS_CONFIG[task.status] || STATUS_CONFIG.open;
  const priorityConf = PRIORITY_CONFIG[task.priority as TaskPriority];

  return (
    <div className="rounded-lg border border-neutral-800 bg-neutral-900/50 overflow-hidden">
      {/* Header */}
      <div
        className="flex items-start gap-3 px-4 py-3 cursor-pointer hover:bg-neutral-800/30 transition-colors"
        onClick={() => setExpanded(!expanded)}
      >
        <button className="mt-1 text-neutral-500 flex-shrink-0">
          {expanded ? <ChevronDown className="w-4 h-4" /> : <ChevronRight className="w-4 h-4" />}
        </button>

        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2 mb-1">
            <Badge variant="outline" className={cn('text-[10px] px-1.5 py-0 border', statusConf.bg, statusConf.color)}>
              {statusConf.label}
            </Badge>
            <span className="text-sm font-medium text-white truncate">{task.title}</span>
          </div>
          {!expanded && task.description && (
            <p className="text-xs text-neutral-500 truncate">{task.description}</p>
          )}
        </div>

        <div className="flex items-center gap-2 flex-shrink-0">
          {priorityConf && (
            <span className={cn('w-2 h-2 rounded-full', priorityConf.dotColor)} title={priorityConf.label} />
          )}
          {task.project && (
            <span className="text-[10px] text-neutral-600 font-mono">{task.project}</span>
          )}
          <span className="text-[10px] text-neutral-600">{timeAgo(task.updatedAt)}</span>
        </div>
      </div>

      {/* Expanded content */}
      {expanded && (
        <div className="px-4 pb-4 border-t border-neutral-800/50">
          {/* Description */}
          {task.description && (
            <div className="mt-3 p-3 rounded-md bg-neutral-950/50 border border-neutral-800/50">
              <p className="text-sm text-neutral-300 whitespace-pre-wrap">{task.description}</p>
            </div>
          )}

          {/* Notes timeline */}
          {notes.length > 0 && (
            <div className="mt-3">
              <div className="text-[10px] text-neutral-600 uppercase tracking-wider mb-2">发现记录</div>
              <div className="space-y-1.5">
                {notes.map((note) => (
                  <div key={note.id} className="flex gap-2 text-xs">
                    <span className="text-neutral-600 flex-shrink-0 w-24 font-mono">{formatDate(note.createdAt)}</span>
                    <span className="text-neutral-400 whitespace-pre-wrap">{note.content}</span>
                  </div>
                ))}
              </div>
            </div>
          )}

          {/* Add note */}
          <div className="mt-3 flex gap-2">
            <Input
              value={newNote}
              onChange={(e) => setNewNote(e.target.value)}
              placeholder="记录发现..."
              className="bg-neutral-800 border-neutral-700 text-white text-xs h-8"
              onKeyDown={(e) => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); addNote(); } }}
            />
            <Button
              size="sm"
              variant="ghost"
              onClick={addNote}
              disabled={!newNote.trim() || submitting}
              className="h-8 px-2 text-cyan-400 hover:text-cyan-300 hover:bg-cyan-500/10"
            >
              <Send className="w-3.5 h-3.5" />
            </Button>
          </div>

          {/* Actions */}
          <div className="mt-3 flex items-center gap-1">
            <Button
              size="sm"
              variant="ghost"
              onClick={(e) => { e.stopPropagation(); cycleStatus(); }}
              className="h-7 px-2 text-xs text-neutral-400 hover:text-white"
            >
              {task.status === 'open' ? <Play className="w-3 h-3 mr-1" /> :
               task.status === 'running' ? <CheckCircle2 className="w-3 h-3 mr-1" /> :
               <Pause className="w-3 h-3 mr-1" />}
              {task.status === 'open' ? '开始研究' : task.status === 'running' ? '标记结论' : '重新开始'}
            </Button>
            <Button
              size="sm"
              variant="ghost"
              onClick={(e) => { e.stopPropagation(); openEditDialog(task); }}
              className="h-7 px-2 text-xs text-neutral-400 hover:text-white"
            >
              <Pencil className="w-3 h-3 mr-1" />
              编辑
            </Button>
            <Button
              size="sm"
              variant="ghost"
              onClick={(e) => { e.stopPropagation(); deleteTask(); }}
              className="h-7 px-2 text-xs text-red-400/60 hover:text-red-400 hover:bg-red-500/10"
            >
              <Trash2 className="w-3 h-3" />
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}

export function ResearchBoard() {
  const tasks = useTaskCenterStore((s) => s.tasks);
  const fetchTasks = useTaskCenterStore((s) => s.fetchTasks);
  const [showDone, setShowDone] = useState(false);
  const [quickTitle, setQuickTitle] = useState('');
  const [creating, setCreating] = useState(false);

  const researchTasks = tasks.filter((t) => t.category === 'research');
  const activeTasks = researchTasks.filter((t) => t.status !== 'done' && t.status !== 'skipped');
  const doneTasks = researchTasks.filter((t) => t.status === 'done' || t.status === 'skipped');

  const quickAdd = async () => {
    if (!quickTitle.trim() || creating) return;
    setCreating(true);
    try {
      await api.createTask({ title: quickTitle.trim(), description: '', priority: 'medium', category: 'research' });
      setQuickTitle('');
      fetchTasks();
    } catch (e) {
      console.error('Failed to create:', e);
    } finally {
      setCreating(false);
    }
  };

  return (
    <div className="flex-1 overflow-auto px-4 sm:px-8 pb-8 max-w-4xl">
      {/* Quick add */}
      <div className="flex gap-2 mb-4">
        <div className="relative flex-1">
          <Plus className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-neutral-600" />
          <Input
            value={quickTitle}
            onChange={(e) => setQuickTitle(e.target.value)}
            placeholder="新研究课题..."
            className="pl-9 bg-neutral-900 border-neutral-800 text-white placeholder:text-neutral-600"
            onKeyDown={(e) => { if (e.key === 'Enter') { e.preventDefault(); quickAdd(); } }}
          />
        </div>
      </div>

      {/* Active research */}
      {activeTasks.length === 0 && doneTasks.length === 0 && (
        <div className="text-center py-16 text-neutral-600 text-sm">
          还没有研究课题，输入标题开始
        </div>
      )}

      <div className="space-y-3">
        {activeTasks.map((task) => (
          <ResearchCard key={task.id} task={task} onRefresh={fetchTasks} />
        ))}
      </div>

      {/* Done section */}
      {doneTasks.length > 0 && (
        <div className="mt-6">
          <button
            onClick={() => setShowDone(!showDone)}
            className="flex items-center gap-2 text-xs text-neutral-600 hover:text-neutral-400 transition-colors mb-2"
          >
            {showDone ? <ChevronDown className="w-3 h-3" /> : <ChevronRight className="w-3 h-3" />}
            已结论 ({doneTasks.length})
          </button>
          {showDone && (
            <div className="space-y-2 opacity-60">
              {doneTasks.map((task) => (
                <ResearchCard key={task.id} task={task} onRefresh={fetchTasks} />
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
