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
import type { TaskCategory, TaskPriority, GroupBy } from '../types';

export function TaskFilters() {
  const filters = useTaskCenterStore((s) => s.filters);
  const setFilters = useTaskCenterStore((s) => s.setFilters);
  const groupBy = useTaskCenterStore((s) => s.groupBy);
  const setGroupBy = useTaskCenterStore((s) => s.setGroupBy);

  return (
    <div className="flex flex-col sm:flex-row gap-2 mb-4">
      <div className="relative flex-1">
        <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-neutral-500" />
        <Input
          value={filters.search}
          onChange={(e) => setFilters({ search: e.target.value })}
          placeholder="搜索..."
          className="bg-neutral-900 border-neutral-800 text-white pl-8 h-8 text-sm"
        />
      </div>

      <Select value={groupBy} onValueChange={(v) => setGroupBy(v as GroupBy)}>
        <SelectTrigger className="bg-neutral-900 border-neutral-800 text-white h-8 text-sm w-full sm:w-[110px]">
          <SelectValue />
        </SelectTrigger>
        <SelectContent className="bg-neutral-800 border-neutral-700">
          {GROUP_OPTIONS.map((opt) => (
            <SelectItem key={opt.value} value={opt.value} className="text-white focus:bg-neutral-700 focus:text-white">
              {opt.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>

      <Select value={filters.category} onValueChange={(v) => setFilters({ category: v as TaskCategory | 'all' })}>
        <SelectTrigger className="bg-neutral-900 border-neutral-800 text-white h-8 text-sm w-full sm:w-[110px]">
          <SelectValue placeholder="全部分类" />
        </SelectTrigger>
        <SelectContent className="bg-neutral-800 border-neutral-700">
          <SelectItem value="all" className="text-neutral-400 focus:bg-neutral-700 focus:text-white">全部分类</SelectItem>
          {(Object.entries(CATEGORY_CONFIG) as [TaskCategory, typeof CATEGORY_CONFIG.dev][]).map(([key, conf]) => (
            <SelectItem key={key} value={key} className="text-white focus:bg-neutral-700 focus:text-white">{conf.label}</SelectItem>
          ))}
        </SelectContent>
      </Select>

      <Select value={filters.priority} onValueChange={(v) => setFilters({ priority: v as TaskPriority | 'all' })}>
        <SelectTrigger className="bg-neutral-900 border-neutral-800 text-white h-8 text-sm w-full sm:w-[110px]">
          <SelectValue placeholder="全部优先级" />
        </SelectTrigger>
        <SelectContent className="bg-neutral-800 border-neutral-700">
          <SelectItem value="all" className="text-neutral-400 focus:bg-neutral-700 focus:text-white">全部优先级</SelectItem>
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
  );
}
