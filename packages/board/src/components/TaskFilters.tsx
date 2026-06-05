import { Search } from 'lucide-react';
import { Input } from '@/components/ui/input';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { useTaskCenterStore } from '../store';
import { CATEGORY_CONFIG, PRIORITY_CONFIG, GROUP_OPTIONS } from '../constants';
import type { TaskCategory, TaskPriority, TaskStatus, GroupBy } from '../types';

const STATUS_OPTIONS: { value: TaskStatus | 'all' | 'active'; label: string }[] = [
  { value: 'active', label: '进行中' },
  { value: 'all', label: '全部状态' },
  { value: 'open', label: '待办' },
  { value: 'running', label: '执行中' },
  { value: 'done', label: '已完成' },
  { value: 'failed', label: '失败' },
  { value: 'blocked', label: '阻塞' },
];

export function TaskFilters() {
  const filters = useTaskCenterStore((s) => s.filters);
  const setFilters = useTaskCenterStore((s) => s.setFilters);
  const groupBy = useTaskCenterStore((s) => s.groupBy);
  const setGroupBy = useTaskCenterStore((s) => s.setGroupBy);

  return (
    <div className="flex flex-col sm:flex-row gap-2 mb-4">
      <div className="relative flex-1">
        <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-stone-500" />
        <Input
          value={filters.search}
          onChange={(e) => setFilters({ search: e.target.value })}
          placeholder="搜索..."
          className="h-8 pl-8 text-sm"
        />
      </div>

      <Select value={groupBy} onValueChange={(v) => setGroupBy(v as GroupBy)}>
        <SelectTrigger className="h-8 w-full text-sm sm:w-[110px]">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {GROUP_OPTIONS.map((opt) => (
            <SelectItem key={opt.value} value={opt.value}>
              {opt.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>

      <Select value={filters.status || 'active'} onValueChange={(v) => setFilters({ status: v as TaskStatus | 'all' | 'active' })}>
        <SelectTrigger className="h-8 w-full text-sm sm:w-[110px]">
          <SelectValue placeholder="进行中" />
        </SelectTrigger>
        <SelectContent>
          {STATUS_OPTIONS.map((opt) => (
            <SelectItem key={opt.value} value={opt.value}>{opt.label}</SelectItem>
          ))}
        </SelectContent>
      </Select>

      <Select value={filters.category} onValueChange={(v) => setFilters({ category: v as TaskCategory | 'all' })}>
        <SelectTrigger className="h-8 w-full text-sm sm:w-[110px]">
          <SelectValue placeholder="全部分类" />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="all" className="text-stone-400">全部分类</SelectItem>
          {(Object.entries(CATEGORY_CONFIG) as [TaskCategory, typeof CATEGORY_CONFIG.dev][]).map(([key, conf]) => (
            <SelectItem key={key} value={key}>{conf.label}</SelectItem>
          ))}
        </SelectContent>
      </Select>

      <Select value={filters.priority} onValueChange={(v) => setFilters({ priority: v as TaskPriority | 'all' })}>
        <SelectTrigger className="h-8 w-full text-sm sm:w-[110px]">
          <SelectValue placeholder="全部优先级" />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="all" className="text-stone-400">全部优先级</SelectItem>
          {(Object.entries(PRIORITY_CONFIG) as [TaskPriority, typeof PRIORITY_CONFIG.high][]).map(([key, conf]) => (
            <SelectItem key={key} value={key}>
              <span className="flex items-center gap-2">
                <span className={`w-2 h-2 rounded-full ${conf.dotColor}`} />
                {conf.label}
              </span>
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </div>
  );
}
