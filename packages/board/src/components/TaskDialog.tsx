import { useEffect, useState } from 'react';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Textarea } from '@/components/ui/textarea';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Trash2, SkipForward } from 'lucide-react';
import { useTaskCenterStore } from '../store';
import { CATEGORY_CONFIG, PRIORITY_CONFIG, SERVER_OPTIONS } from '../constants';
import type { TaskFormData, TaskCategory, TaskPriority } from '../types';

const defaultForm: TaskFormData = {
  title: '',
  description: '',
  priority: 'medium',
  category: 'other',
  project: '',
  server: undefined,
  dueDate: undefined,
};

export function TaskDialog() {
  const isDialogOpen = useTaskCenterStore((s) => s.isDialogOpen);
  const editingTask = useTaskCenterStore((s) => s.editingTask);
  const addTask = useTaskCenterStore((s) => s.addTask);
  const updateTask = useTaskCenterStore((s) => s.updateTask);
  const deleteTask = useTaskCenterStore((s) => s.deleteTask);
  const skipTask = useTaskCenterStore((s) => s.skipTask);
  const closeDialog = useTaskCenterStore((s) => s.closeDialog);
  const parentId = useTaskCenterStore((s) => s._addDialogParentId);

  const [form, setForm] = useState<TaskFormData>(defaultForm);
  const isEditing = !!editingTask;

  useEffect(() => {
    if (editingTask) {
      setForm({
        title: editingTask.title,
        description: editingTask.description,
        priority: editingTask.priority,
        category: editingTask.category,
        project: editingTask.project || '',
        server: editingTask.server,
        dueDate: editingTask.dueDate,
        hidden: editingTask.hidden,
      });
    } else {
      setForm(defaultForm);
    }
  }, [editingTask, isDialogOpen]);

  const handleSave = () => {
    if (!form.title.trim()) return;
    const data: TaskFormData = {
      ...form,
      project: form.project?.trim() || undefined,
      server: form.server || undefined,
      dueDate: form.dueDate || undefined,
      hidden: form.hidden || undefined,
    };
    if (isEditing) {
      updateTask(editingTask.id, data);
    } else {
      addTask(data, parentId);
    }
  };

  const handleDelete = () => {
    if (editingTask) deleteTask(editingTask.id);
  };

  return (
    <Dialog open={isDialogOpen} onOpenChange={(open) => !open && closeDialog()}>
      <DialogContent className="bg-neutral-900 border-neutral-800 text-white sm:max-w-[480px]">
        <DialogHeader>
          <DialogTitle>{isEditing ? '编辑任务' : '新建任务'}</DialogTitle>
        </DialogHeader>

        <div className="space-y-4 py-2">
          <div className="space-y-1.5">
            <Label className="text-neutral-400 text-xs">标题</Label>
            <Input
              value={form.title}
              onChange={(e) => setForm({ ...form, title: e.target.value })}
              placeholder="任务标题"
              className="bg-neutral-800 border-neutral-700 text-white"
              autoFocus
            />
          </div>

          <div className="space-y-1.5">
            <Label className="text-neutral-400 text-xs">描述</Label>
            <Textarea
              value={form.description}
              onChange={(e) => setForm({ ...form, description: e.target.value })}
              placeholder="补充说明（可选）"
              className="bg-neutral-800 border-neutral-700 text-white resize-none"
              rows={2}
            />
          </div>

          <div className="grid grid-cols-2 gap-3">
            <div className="space-y-1.5">
              <Label className="text-neutral-400 text-xs">优先级</Label>
              <Select value={form.priority} onValueChange={(v) => setForm({ ...form, priority: v as TaskPriority })}>
                <SelectTrigger className="bg-neutral-800 border-neutral-700 text-white">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent className="bg-neutral-800 border-neutral-700">
                  {(Object.entries(PRIORITY_CONFIG) as [TaskPriority, typeof PRIORITY_CONFIG.high][]).map(([key, conf]) => (
                    <SelectItem key={key} value={key} className="text-white focus:bg-neutral-700 focus:text-white">
                      <span className="flex items-center gap-2">
                        <span className={`w-2 h-2 rounded-full ${conf.dotColor}`} />
                        {conf.label}
                      </span>
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>

            <div className="space-y-1.5">
              <Label className="text-neutral-400 text-xs">分类</Label>
              <Select value={form.category} onValueChange={(v) => setForm({ ...form, category: v as TaskCategory })}>
                <SelectTrigger className="bg-neutral-800 border-neutral-700 text-white">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent className="bg-neutral-800 border-neutral-700">
                  {(Object.entries(CATEGORY_CONFIG) as [TaskCategory, typeof CATEGORY_CONFIG.dev][]).map(([key, conf]) => (
                    <SelectItem key={key} value={key} className="text-white focus:bg-neutral-700 focus:text-white">{conf.label}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          </div>

          <div className="grid grid-cols-2 gap-3">
            <div className="space-y-1.5">
              <Label className="text-neutral-400 text-xs">关联项目</Label>
              <Input
                value={form.project || ''}
                onChange={(e) => setForm({ ...form, project: e.target.value })}
                placeholder="项目名（可选）"
                className="bg-neutral-800 border-neutral-700 text-white"
              />
            </div>

            <div className="space-y-1.5">
              <Label className="text-neutral-400 text-xs">服务器</Label>
              <Select value={form.server || '_none'} onValueChange={(v) => setForm({ ...form, server: v === '_none' ? undefined : v })}>
                <SelectTrigger className="bg-neutral-800 border-neutral-700 text-white">
                  <SelectValue placeholder="无" />
                </SelectTrigger>
                <SelectContent className="bg-neutral-800 border-neutral-700">
                  <SelectItem value="_none" className="text-neutral-400 focus:bg-neutral-700 focus:text-white">无</SelectItem>
                  {SERVER_OPTIONS.map((s) => (
                    <SelectItem key={s} value={s} className="text-white focus:bg-neutral-700 focus:text-white">{s}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          </div>

          <div className="space-y-1.5">
            <Label className="text-neutral-400 text-xs">截止日期</Label>
            <Input
              type="date"
              value={form.dueDate || ''}
              onChange={(e) => setForm({ ...form, dueDate: e.target.value || undefined })}
              className="bg-neutral-800 border-neutral-700 text-white"
            />
          </div>

          <label className="flex items-center gap-2 cursor-pointer">
            <input
              type="checkbox"
              checked={form.hidden || false}
              onChange={(e) => setForm({ ...form, hidden: e.target.checked })}
              className="rounded border-neutral-700 bg-neutral-800 text-orange-500 focus:ring-orange-500/20"
            />
            <span className="text-xs text-neutral-400">隐藏任务（手动处理项，不在默认列表显示）</span>
          </label>
        </div>

        <DialogFooter className="flex-row justify-between sm:justify-between">
          {isEditing ? (
            <div className="flex gap-1">
              <Button variant="ghost" size="sm" onClick={handleDelete} className="text-red-400 hover:text-red-300 hover:bg-red-500/10">
                <Trash2 className="w-4 h-4 mr-1" />
                删除
              </Button>
              {editingTask.status !== 'done' && (
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => { skipTask(editingTask.id); closeDialog(); }}
                  className="text-neutral-400 hover:text-neutral-300 hover:bg-neutral-500/10"
                >
                  <SkipForward className="w-4 h-4 mr-1" />
                  {editingTask.status === 'skipped' ? '恢复' : '跳过'}
                </Button>
              )}
            </div>
          ) : (
            <div />
          )}
          <div className="flex gap-2">
            <Button variant="ghost" onClick={closeDialog}>取消</Button>
            <Button onClick={handleSave} disabled={!form.title.trim()}>保存</Button>
          </div>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
