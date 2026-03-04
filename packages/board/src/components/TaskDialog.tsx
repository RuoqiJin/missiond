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
import { CATEGORY_CONFIG, PRIORITY_CONFIG, SERVER_OPTIONS, SLOT_OPTIONS, FLOW_TEMPLATE_OPTIONS } from '../constants';
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
  const [autopilotOpen, setAutopilotOpen] = useState(false);
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
        assignee: editingTask.assignee,
        autoExecute: editingTask.autoExecute,
        promptTemplate: editingTask.promptTemplate,
        flowTemplate: editingTask.flowTemplate,
        dependsOn: editingTask.dependsOn,
      });
      setAutopilotOpen(!!(editingTask.autoExecute || editingTask.assignee || editingTask.flowTemplate));
    } else {
      setForm(defaultForm);
      setAutopilotOpen(false);
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
      // Autopilot fields: only include if section is open
      assignee: autopilotOpen ? (form.assignee || undefined) : undefined,
      autoExecute: autopilotOpen ? form.autoExecute : undefined,
      promptTemplate: autopilotOpen ? (form.promptTemplate?.trim() || undefined) : undefined,
      flowTemplate: autopilotOpen ? (form.flowTemplate || undefined) : undefined,
      dependsOn: autopilotOpen && form.dependsOn?.length ? form.dependsOn : undefined,
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

          {/* Autopilot Toggle */}
          <div className="border-t border-neutral-800 pt-3">
            <label className="flex items-center gap-2 cursor-pointer">
              <input
                type="checkbox"
                checked={autopilotOpen}
                onChange={(e) => {
                  setAutopilotOpen(e.target.checked);
                  if (e.target.checked) {
                    setForm({ ...form, autoExecute: true });
                  }
                }}
                className="rounded border-neutral-700 bg-neutral-800 text-cyan-500 focus:ring-cyan-500/20"
              />
              <span className="text-xs text-cyan-400">⚡ 启用无人值守 (Autopilot)</span>
            </label>

            {/* Autopilot Section — Progressive Disclosure */}
            {autopilotOpen && (
              <div className="mt-3 space-y-3 p-3 rounded-md bg-neutral-800/50 border border-cyan-500/10">
                <div className="grid grid-cols-2 gap-3">
                  <div className="space-y-1.5">
                    <Label className="text-neutral-400 text-xs">工位分配</Label>
                    <Select
                      value={form.assignee || '_none'}
                      onValueChange={(v) => setForm({ ...form, assignee: v === '_none' ? undefined : v })}
                    >
                      <SelectTrigger className="bg-neutral-800 border-neutral-700 text-white font-mono text-xs">
                        <SelectValue placeholder="选择工位" />
                      </SelectTrigger>
                      <SelectContent className="bg-neutral-800 border-neutral-700">
                        <SelectItem value="_none" className="text-neutral-400 focus:bg-neutral-700 focus:text-white">未分配</SelectItem>
                        {SLOT_OPTIONS.map((s) => (
                          <SelectItem key={s} value={s} className="text-white focus:bg-neutral-700 focus:text-white font-mono text-xs">{s}</SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </div>

                  <div className="space-y-1.5">
                    <Label className="text-neutral-400 text-xs">工作流引擎</Label>
                    <Select
                      value={form.flowTemplate || ''}
                      onValueChange={(v) => setForm({ ...form, flowTemplate: v || undefined })}
                    >
                      <SelectTrigger className="bg-neutral-800 border-neutral-700 text-white text-xs">
                        <SelectValue placeholder="无" />
                      </SelectTrigger>
                      <SelectContent className="bg-neutral-800 border-neutral-700">
                        {FLOW_TEMPLATE_OPTIONS.map((o) => (
                          <SelectItem key={o.value} value={o.value || '_none'} className="text-white focus:bg-neutral-700 focus:text-white text-xs">{o.label}</SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </div>
                </div>

                <div className="space-y-1.5">
                  <Label className="text-neutral-400 text-xs">前置依赖（Task ID，逗号分隔）</Label>
                  <Input
                    value={form.dependsOn?.join(', ') || ''}
                    onChange={(e) => {
                      const ids = e.target.value.split(',').map(s => s.trim()).filter(Boolean);
                      setForm({ ...form, dependsOn: ids.length ? ids : undefined });
                    }}
                    placeholder="如: abc123, def456"
                    className="bg-neutral-800 border-neutral-700 text-white font-mono text-xs"
                  />
                </div>

                <div className="space-y-1.5">
                  <Label className="text-neutral-400 text-xs">
                    执行提示词
                    <span className="ml-2 text-neutral-500">留空则使用标题+描述</span>
                  </Label>
                  <Textarea
                    value={form.promptTemplate || ''}
                    onChange={(e) => setForm({ ...form, promptTemplate: e.target.value })}
                    placeholder="覆盖下发给 Claude 的指令（可选）"
                    className="bg-neutral-800 border-neutral-700 text-white resize-none font-mono text-xs"
                    rows={3}
                  />
                </div>
              </div>
            )}
          </div>
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
