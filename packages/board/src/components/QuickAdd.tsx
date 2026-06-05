import { useState } from 'react';
import { Plus } from 'lucide-react';
import { useTaskCenterStore } from '../store';

export function QuickAdd() {
  const [value, setValue] = useState('');
  const quickAdd = useTaskCenterStore((s) => s.quickAdd);

  const handleSubmit = () => {
    const trimmed = value.trim();
    if (!trimmed) return;
    quickAdd(trimmed);
    setValue('');
  };

  return (
    <div className="mission-control flex items-center gap-2 px-3 py-2 transition-colors">
      <Plus className="w-4 h-4 text-stone-500 shrink-0" />
      <input
        value={value}
        onChange={(e) => setValue(e.target.value)}
        onKeyDown={(e) => e.key === 'Enter' && handleSubmit()}
        placeholder="写下你的想法，回车添加..."
        className="flex-1 bg-transparent text-sm text-stone-100 placeholder:text-stone-500 outline-none"
      />
    </div>
  );
}
