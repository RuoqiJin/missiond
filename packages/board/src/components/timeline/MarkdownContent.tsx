import { memo } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';

export const MarkdownContent = memo(function MarkdownContent({ content }: { content: string }) {
  return (
    <div className="prose prose-sm prose-invert max-w-none
      prose-headings:text-teal-200 prose-headings:font-semibold prose-headings:mt-3 prose-headings:mb-1
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
  );
});
