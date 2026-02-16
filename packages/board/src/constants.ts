import type { TaskCategory, TaskPriority, GroupBy } from './types';

export const CATEGORY_CONFIG: Record<TaskCategory, { label: string; className: string }> = {
  deploy: { label: '部署', className: 'bg-orange-500/10 text-orange-400 border-orange-500/20' },
  dev: { label: '开发', className: 'bg-blue-500/10 text-blue-400 border-blue-500/20' },
  infra: { label: '基建', className: 'bg-purple-500/10 text-purple-400 border-purple-500/20' },
  test: { label: '测试', className: 'bg-green-500/10 text-green-400 border-green-500/20' },
  other: { label: '其他', className: 'bg-neutral-500/10 text-neutral-400 border-neutral-500/20' },
};

export const PRIORITY_CONFIG: Record<TaskPriority, { label: string; dotColor: string }> = {
  high: { label: '高', dotColor: 'bg-red-500' },
  medium: { label: '中', dotColor: 'bg-yellow-500' },
  low: { label: '低', dotColor: 'bg-blue-500' },
};

export const GROUP_OPTIONS: { value: GroupBy; label: string }[] = [
  { value: 'none', label: '不分组' },
  { value: 'category', label: '按分类' },
  { value: 'priority', label: '按优先级' },
  { value: 'project', label: '按项目' },
];

export const SERVER_OPTIONS = ['私有云', 'ECS', 'GCP', 'Win Agent'] as const;
