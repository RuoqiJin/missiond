'use client';

import { useState, useEffect, useCallback } from 'react';
import { FileText, ArrowLeft, RefreshCw, FolderOpen } from 'lucide-react';
import { cn } from '@/lib/utils';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';

interface MdFile {
  name: string;
  path: string;
  size: number;
  mtime: string;
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes}B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)}KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)}MB`;
}

function formatDate(iso: string): string {
  return new Date(iso).toLocaleString('zh-CN', {
    month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit',
  });
}

function dirLabel(path: string): string {
  if (path.includes('/Desktop/')) return 'Desktop';
  if (path.includes('/Projects/missiond/')) return 'Project';
  return 'Other';
}

export function TranscriptViewer() {
  const [files, setFiles] = useState<MdFile[]>([]);
  const [loading, setLoading] = useState(true);
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [content, setContent] = useState<string>('');
  const [contentLoading, setContentLoading] = useState(false);

  const fetchList = useCallback(() => {
    setLoading(true);
    fetch('/api/transcripts')
      .then((r) => r.json())
      .then((data) => setFiles(data.files || []))
      .catch(() => setFiles([]))
      .finally(() => setLoading(false));
  }, []);

  useEffect(() => { fetchList(); }, [fetchList]);

  const openFile = useCallback((path: string) => {
    setSelectedPath(path);
    setContentLoading(true);
    fetch(`/api/transcripts?path=${encodeURIComponent(path)}`)
      .then((r) => r.json())
      .then((data) => setContent(data.content || ''))
      .catch(() => setContent('Failed to load file'))
      .finally(() => setContentLoading(false));
  }, []);

  // File list view
  if (!selectedPath) {
    return (
      <div className="flex-1 flex flex-col overflow-hidden">
        <div className="px-4 sm:px-8 py-3 flex items-center gap-3">
          <FolderOpen className="w-4 h-4 text-neutral-500" />
          <span className="text-sm text-neutral-400">{files.length} files</span>
          <button onClick={fetchList} className="ml-auto text-neutral-600 hover:text-neutral-400 transition-colors">
            <RefreshCw className={cn('w-3.5 h-3.5', loading && 'animate-spin')} />
          </button>
        </div>
        <div className="flex-1 overflow-y-auto px-4 sm:px-8 pb-4">
          {loading && files.length === 0 ? (
            <div className="text-neutral-600 text-sm py-8 text-center">Loading...</div>
          ) : files.length === 0 ? (
            <div className="text-neutral-600 text-sm py-8 text-center">No MD files found</div>
          ) : (
            <div className="space-y-1">
              {files.map((f) => (
                <button
                  key={f.path}
                  onClick={() => openFile(f.path)}
                  className="w-full text-left px-3 py-2.5 rounded-lg hover:bg-neutral-800/50 transition-colors group flex items-center gap-3"
                >
                  <FileText className="w-4 h-4 text-neutral-600 group-hover:text-neutral-400 flex-shrink-0" />
                  <div className="flex-1 min-w-0">
                    <div className="text-sm text-neutral-300 group-hover:text-white truncate">{f.name}</div>
                    <div className="text-[11px] text-neutral-600 flex items-center gap-2 mt-0.5">
                      <span>{dirLabel(f.path)}</span>
                      <span>{formatSize(f.size)}</span>
                      <span>{formatDate(f.mtime)}</span>
                    </div>
                  </div>
                </button>
              ))}
            </div>
          )}
        </div>
      </div>
    );
  }

  // Content view
  const fileName = files.find((f) => f.path === selectedPath)?.name || selectedPath.split('/').pop() || '';

  return (
    <div className="flex-1 flex flex-col overflow-hidden">
      {/* Header */}
      <div className="px-4 sm:px-8 py-3 flex items-center gap-3 border-b border-neutral-800/50">
        <button
          onClick={() => { setSelectedPath(null); setContent(''); }}
          className="text-neutral-500 hover:text-neutral-300 transition-colors"
        >
          <ArrowLeft className="w-4 h-4" />
        </button>
        <FileText className="w-4 h-4 text-neutral-500" />
        <span className="text-sm text-neutral-300 font-medium truncate">{fileName}</span>
      </div>

      {/* Content */}
      <div className="flex-1 overflow-y-auto px-4 sm:px-8 py-4">
        {contentLoading ? (
          <div className="text-neutral-600 text-sm py-8 text-center">Loading...</div>
        ) : (
          <div className="max-w-4xl mx-auto prose prose-sm prose-invert
            prose-headings:text-teal-200 prose-headings:font-semibold prose-headings:mt-4 prose-headings:mb-2
            prose-p:text-teal-100/90 prose-p:my-1.5 prose-p:leading-relaxed
            prose-strong:text-teal-200 prose-em:text-teal-200/80
            prose-li:text-teal-100/90 prose-li:my-0.5
            prose-ul:my-1.5 prose-ol:my-1.5
            prose-a:text-cyan-400 prose-a:no-underline hover:prose-a:underline
            prose-code:text-amber-300 prose-code:bg-neutral-800 prose-code:px-1 prose-code:py-0.5 prose-code:rounded prose-code:text-xs prose-code:before:content-none prose-code:after:content-none
            prose-pre:bg-neutral-900 prose-pre:border prose-pre:border-neutral-800 prose-pre:rounded-md prose-pre:my-2
            prose-blockquote:border-teal-500/30 prose-blockquote:text-teal-200/70
            prose-hr:border-neutral-700
            prose-table:text-xs prose-table:w-full prose-table:border-collapse prose-table:my-2
            prose-thead:border-b prose-thead:border-neutral-700
            prose-th:text-teal-300 prose-th:text-left prose-th:px-3 prose-th:py-1.5 prose-th:bg-neutral-800/50 prose-th:border prose-th:border-neutral-700/60 prose-th:font-medium
            prose-td:text-teal-100/80 prose-td:px-3 prose-td:py-1.5 prose-td:border prose-td:border-neutral-700/40
          ">
            <ReactMarkdown remarkPlugins={[remarkGfm]}>{content}</ReactMarkdown>
          </div>
        )}
      </div>
    </div>
  );
}
