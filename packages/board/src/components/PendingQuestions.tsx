'use client';

import { useState, useEffect, useCallback } from 'react';
import { HelpCircle, ChevronDown, ChevronRight, Send, X } from 'lucide-react';
import type { AgentQuestion } from '../types';
import * as questionsApi from '../questionsApi';

export function PendingQuestions() {
  const [questions, setQuestions] = useState<AgentQuestion[]>([]);
  const [isExpanded, setIsExpanded] = useState(true);
  const [answerInputs, setAnswerInputs] = useState<Record<string, string>>({});

  const fetchPending = useCallback(async () => {
    try {
      const qs = await questionsApi.fetchQuestions('pending');
      setQuestions(qs);
    } catch {
      // silent — missiond may be unavailable
    }
  }, []);

  useEffect(() => {
    fetchPending();
    const interval = setInterval(fetchPending, 30_000);
    return () => clearInterval(interval);
  }, [fetchPending]);

  const handleAnswer = async (id: string) => {
    const answer = answerInputs[id]?.trim();
    if (!answer) return;
    setQuestions((prev) => prev.filter((q) => q.id !== id));
    setAnswerInputs((prev) => {
      const next = { ...prev };
      delete next[id];
      return next;
    });
    await questionsApi.answerQuestion(id, answer);
  };

  const handleDismiss = async (id: string) => {
    setQuestions((prev) => prev.filter((q) => q.id !== id));
    await questionsApi.dismissQuestion(id);
  };

  if (questions.length === 0) return null;

  return (
    <div className="mb-4 border border-amber-500/20 rounded-lg bg-amber-500/5">
      <button
        onClick={() => setIsExpanded(!isExpanded)}
        className="flex items-center gap-2 w-full px-3 py-2 text-xs text-amber-400 hover:text-amber-300 transition-colors"
      >
        <HelpCircle className="w-3.5 h-3.5" />
        <span className="font-medium">Agent 待决策 ({questions.length})</span>
        {isExpanded ? <ChevronDown className="w-3 h-3 ml-auto" /> : <ChevronRight className="w-3 h-3 ml-auto" />}
      </button>
      {isExpanded && (
        <div className="px-3 pb-3 space-y-2">
          {questions.map((q) => (
            <div key={q.id} className="p-2.5 rounded bg-neutral-900/50 border border-neutral-800">
              <div className="flex items-start justify-between gap-2">
                <p className="text-sm text-neutral-200">{q.question}</p>
                {q.slotId && (
                  <span className="text-[10px] px-1.5 py-0.5 rounded bg-neutral-800 text-neutral-500 shrink-0 font-mono">
                    {q.slotId}
                  </span>
                )}
              </div>
              {q.context && (
                <p className="mt-1.5 text-xs text-neutral-500 whitespace-pre-wrap">{q.context}</p>
              )}
              <div className="mt-2 flex gap-1.5">
                <input
                  className="flex-1 px-2 py-1 text-xs bg-neutral-800 border border-neutral-700 rounded text-neutral-200 placeholder:text-neutral-600 outline-none focus:border-amber-500/50"
                  placeholder="输入回答..."
                  value={answerInputs[q.id] || ''}
                  onChange={(e) => setAnswerInputs((prev) => ({ ...prev, [q.id]: e.target.value }))}
                  onKeyDown={(e) => e.key === 'Enter' && handleAnswer(q.id)}
                />
                <button
                  onClick={() => handleAnswer(q.id)}
                  className="p-1.5 text-amber-400 hover:text-amber-300 hover:bg-amber-500/10 rounded transition-colors"
                  title="回答"
                >
                  <Send className="w-3.5 h-3.5" />
                </button>
                <button
                  onClick={() => handleDismiss(q.id)}
                  className="p-1.5 text-neutral-600 hover:text-neutral-400 hover:bg-neutral-800 rounded transition-colors"
                  title="忽略"
                >
                  <X className="w-3.5 h-3.5" />
                </button>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
