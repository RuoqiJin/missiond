'use client';

import { useState, useRef, useEffect, useCallback } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import {
  Send, ImagePlus, Loader2, Wrench, Brain, X, Plus,
  MessageSquare, ChevronLeft,
} from 'lucide-react';
import { cn } from '@/lib/utils';

const WS_PORT = parseInt(process.env.NEXT_PUBLIC_WS_PORT || '9120', 10);

// ─── Types ───

interface ChatMessage {
  id: string;
  role: 'user' | 'assistant' | 'system';
  content: string;
  /** base64 image data URLs */
  images?: string[];
  timestamp: number;
}

interface Conversation {
  id: string;
  title: string;
  updatedAt: string;
  messageCount: number;
}

interface ToolActivity {
  id: string;
  tool: string;
  params?: string;
  status: 'running' | 'done';
  durationMs?: number;
}

// ─── SSE Parser ───

function parseSSE(chunk: string): Array<{ event?: string; data: string }> {
  const events: Array<{ event?: string; data: string }> = [];
  let currentEvent: string | undefined;
  let dataLines: string[] = [];

  for (const line of chunk.split('\n')) {
    if (line.startsWith('event: ')) {
      currentEvent = line.slice(7).trim();
    } else if (line.startsWith('data: ')) {
      dataLines.push(line.slice(6));
    } else if (line === '' && dataLines.length > 0) {
      events.push({ event: currentEvent, data: dataLines.join('\n') });
      currentEvent = undefined;
      dataLines = [];
    }
  }
  // Handle trailing data without final newline
  if (dataLines.length > 0) {
    events.push({ event: currentEvent, data: dataLines.join('\n') });
  }
  return events;
}

// ─── Main Component ───

export function JarvisChat() {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = useState('');
  const [images, setImages] = useState<string[]>([]);
  const [isStreaming, setIsStreaming] = useState(false);
  const [statusText, setStatusText] = useState('');
  const [tools, setTools] = useState<ToolActivity[]>([]);
  const [error, setError] = useState<string | null>(null);

  // Conversation history sidebar
  const [conversations, setConversations] = useState<Conversation[]>([]);
  const [activeConvId, setActiveConvId] = useState<string | null>(null);
  const [sidebarOpen, setSidebarOpen] = useState(false);

  const messagesEndRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const abortRef = useRef<AbortController | null>(null);

  // Auto-scroll to bottom on new messages
  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages, statusText, tools]);

  // Load conversation list
  const loadConversations = useCallback(async () => {
    try {
      const res = await fetch('/api/jarvis/conversations');
      if (res.ok) {
        const data = await res.json();
        setConversations(data);
      }
    } catch { /* ignore */ }
  }, []);

  useEffect(() => { loadConversations(); }, [loadConversations]);

  // Load a specific conversation's messages
  const loadConversation = useCallback(async (convId: string) => {
    try {
      const res = await fetch(`/api/jarvis/conversations?id=${convId}`);
      if (res.ok) {
        const data = await res.json();
        const msgs: ChatMessage[] = (data.messages || []).map((m: { id: number; role: string; content: string; timestamp: string }) => ({
          id: String(m.id),
          role: m.role as 'user' | 'assistant',
          content: m.content,
          timestamp: new Date(m.timestamp).getTime(),
        }));
        setMessages(msgs);
        setActiveConvId(convId);
        setSidebarOpen(false);
      }
    } catch { /* ignore */ }
  }, []);

  // New conversation
  const startNewChat = useCallback(() => {
    setMessages([]);
    setActiveConvId(null);
    setInput('');
    setImages([]);
    setError(null);
    setSidebarOpen(false);
    inputRef.current?.focus();
  }, []);

  // Image handling
  const handleImageUpload = useCallback((files: FileList | null) => {
    if (!files) return;
    Array.from(files).forEach((file) => {
      if (!file.type.startsWith('image/')) return;
      if (file.size > 10 * 1024 * 1024) return; // 10MB limit
      const reader = new FileReader();
      reader.onload = (e) => {
        const dataUrl = e.target?.result as string;
        setImages((prev) => [...prev, dataUrl]);
      };
      reader.readAsDataURL(file);
    });
  }, []);

  const removeImage = useCallback((index: number) => {
    setImages((prev) => prev.filter((_, i) => i !== index));
  }, []);

  // Handle paste for images
  const handlePaste = useCallback((e: React.ClipboardEvent) => {
    const items = e.clipboardData?.items;
    if (!items) return;
    const imageFiles: File[] = [];
    for (const item of Array.from(items)) {
      if (item.type.startsWith('image/')) {
        const file = item.getAsFile();
        if (file) imageFiles.push(file);
      }
    }
    if (imageFiles.length > 0) {
      e.preventDefault();
      const dt = new DataTransfer();
      imageFiles.forEach((f) => dt.items.add(f));
      handleImageUpload(dt.files);
    }
  }, [handleImageUpload]);

  // Handle drop for images
  const handleDrop = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    handleImageUpload(e.dataTransfer.files);
  }, [handleImageUpload]);

  // Send message
  const sendMessage = useCallback(async () => {
    const text = input.trim();
    if (!text && images.length === 0) return;
    if (isStreaming) return;

    // Build user message
    const userMsg: ChatMessage = {
      id: `user-${Date.now()}`,
      role: 'user',
      content: text,
      images: images.length > 0 ? [...images] : undefined,
      timestamp: Date.now(),
    };
    setMessages((prev) => [...prev, userMsg]);
    setInput('');
    setImages([]);
    setIsStreaming(true);
    setStatusText('');
    setTools([]);
    setError(null);

    // Build OpenAI-compatible messages
    const apiMessages = messages
      .filter((m) => m.role !== 'system')
      .concat(userMsg)
      .map((m) => {
        if (m.images && m.images.length > 0) {
          return {
            role: m.role,
            content: [
              { type: 'text' as const, text: m.content },
              ...m.images.map((img) => ({
                type: 'image_url' as const,
                image_url: { url: img },
              })),
            ],
          };
        }
        return { role: m.role, content: m.content };
      });

    const controller = new AbortController();
    abortRef.current = controller;

    try {
      const wsHost = typeof window !== 'undefined' ? window.location.hostname : 'localhost';
      const isLocal = wsHost === 'localhost' || wsHost === '127.0.0.1';
      const port = isLocal ? WS_PORT : (parseInt(window.location.port, 10) || WS_PORT);
      const baseUrl = `http://${wsHost}:${port}`;

      const res = await fetch(`${baseUrl}/v1/chat/completions`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'Authorization': 'Bearer jarvis-ui',
          'X-Slot-Id': 'slot-jarvis',
        },
        body: JSON.stringify({ messages: apiMessages }),
        signal: controller.signal,
      });

      if (!res.ok) {
        const errBody = await res.text();
        try {
          const parsed = JSON.parse(errBody);
          throw new Error(parsed.error?.message || `HTTP ${res.status}`);
        } catch {
          throw new Error(`HTTP ${res.status}: ${errBody.slice(0, 200)}`);
        }
      }

      // Parse SSE stream
      const reader = res.body?.getReader();
      if (!reader) throw new Error('No response body');

      const decoder = new TextDecoder();
      let buffer = '';
      let assistantContent = '';

      while (true) {
        const { done, value } = await reader.read();
        if (done) break;

        buffer += decoder.decode(value, { stream: true });
        const events = parseSSE(buffer);
        buffer = '';

        for (const evt of events) {
          if (evt.data === '[DONE]') continue;

          if (evt.event === 'status') {
            try {
              const status = JSON.parse(evt.data);
              setStatusText(status.text || status.phase || '');
            } catch { /* ignore */ }
            continue;
          }

          if (evt.event === 'tool_start') {
            try {
              const tool = JSON.parse(evt.data);
              setTools((prev) => [...prev, {
                id: tool.id, tool: tool.tool,
                params: tool.params, status: 'running',
              }]);
            } catch { /* ignore */ }
            continue;
          }

          if (evt.event === 'tool_end') {
            try {
              const tool = JSON.parse(evt.data);
              setTools((prev) => prev.map((t) =>
                t.id === tool.id ? { ...t, status: 'done' as const, durationMs: tool.duration_ms } : t
              ));
            } catch { /* ignore */ }
            continue;
          }

          // Default: chat completion chunk
          if (!evt.event) {
            try {
              const chunk = JSON.parse(evt.data);
              if (chunk.error) {
                setError(chunk.error.message);
                continue;
              }
              const delta = chunk.choices?.[0]?.delta;
              if (delta?.content) {
                assistantContent += delta.content;
                setMessages((prev) => {
                  const existing = prev.find((m) => m.id === 'streaming');
                  if (existing) {
                    return prev.map((m) =>
                      m.id === 'streaming' ? { ...m, content: assistantContent } : m
                    );
                  }
                  return [...prev, {
                    id: 'streaming',
                    role: 'assistant' as const,
                    content: assistantContent,
                    timestamp: Date.now(),
                  }];
                });
              }
              if (chunk.choices?.[0]?.finish_reason === 'stop') {
                // Finalize the streaming message
                setMessages((prev) =>
                  prev.map((m) =>
                    m.id === 'streaming' ? { ...m, id: `asst-${Date.now()}` } : m
                  )
                );
              }
            } catch { /* ignore malformed JSON */ }
          }
        }
      }

      // Save conversation
      if (assistantContent) {
        try {
          await fetch('/api/jarvis/conversations', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({
              conversationId: activeConvId,
              userMessage: text,
              assistantMessage: assistantContent,
            }),
          });
          loadConversations();
        } catch { /* ignore save errors */ }
      }
    } catch (err) {
      if ((err as Error).name === 'AbortError') {
        // User cancelled
      } else {
        setError((err as Error).message);
      }
    } finally {
      setIsStreaming(false);
      setStatusText('');
      abortRef.current = null;
    }
  }, [input, images, messages, isStreaming, activeConvId, loadConversations]);

  // Stop streaming
  const handleStop = useCallback(() => {
    abortRef.current?.abort();
    setIsStreaming(false);
    setStatusText('');
  }, []);

  // Keyboard handling
  const handleKeyDown = useCallback((e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      sendMessage();
    }
  }, [sendMessage]);

  // Auto-resize textarea
  const adjustTextareaHeight = useCallback(() => {
    const el = inputRef.current;
    if (el) {
      el.style.height = 'auto';
      el.style.height = Math.min(el.scrollHeight, 200) + 'px';
    }
  }, []);

  return (
    <div className="flex-1 flex min-h-0 mx-4 sm:mx-8 mb-4"
      onDragOver={(e) => e.preventDefault()}
      onDrop={handleDrop}
    >
      {/* Sidebar: conversation history */}
      {sidebarOpen && (
        <div className="w-64 flex-shrink-0 border-r border-neutral-800 flex flex-col bg-neutral-950 rounded-l-lg">
          <div className="flex items-center justify-between p-3 border-b border-neutral-800">
            <span className="text-xs font-medium text-neutral-400">History</span>
            <div className="flex gap-1">
              <button onClick={startNewChat} className="p-1 text-neutral-500 hover:text-white transition-colors" title="New chat">
                <Plus className="w-3.5 h-3.5" />
              </button>
              <button onClick={() => setSidebarOpen(false)} className="p-1 text-neutral-500 hover:text-white transition-colors">
                <ChevronLeft className="w-3.5 h-3.5" />
              </button>
            </div>
          </div>
          <div className="flex-1 overflow-y-auto">
            {conversations.map((conv) => (
              <button
                key={conv.id}
                onClick={() => loadConversation(conv.id)}
                className={cn(
                  'w-full text-left px-3 py-2 text-xs border-b border-neutral-900 hover:bg-neutral-900 transition-colors',
                  activeConvId === conv.id ? 'bg-neutral-800/50 text-white' : 'text-neutral-400',
                )}
              >
                <div className="truncate">{conv.title}</div>
                <div className="text-[10px] text-neutral-600 mt-0.5">
                  {conv.messageCount} messages
                </div>
              </button>
            ))}
            {conversations.length === 0 && (
              <div className="p-4 text-center text-neutral-600 text-xs">No conversations yet</div>
            )}
          </div>
        </div>
      )}

      {/* Main chat area */}
      <div className="flex-1 flex flex-col min-w-0 border border-neutral-800 rounded-lg overflow-hidden"
        style={sidebarOpen ? { borderTopLeftRadius: 0, borderBottomLeftRadius: 0 } : undefined}
      >
        {/* Chat header */}
        <div className="flex items-center justify-between px-4 py-2 border-b border-neutral-800 bg-neutral-950">
          <div className="flex items-center gap-2">
            {!sidebarOpen && (
              <button onClick={() => setSidebarOpen(true)} className="p-1 text-neutral-500 hover:text-white transition-colors" title="History">
                <MessageSquare className="w-4 h-4" />
              </button>
            )}
            <span className="text-sm font-medium text-neutral-300">Jarvis</span>
            {isStreaming && (
              <span className="flex items-center gap-1 text-[10px] text-amber-400">
                <Loader2 className="w-3 h-3 animate-spin" />
                {statusText || 'Processing...'}
              </span>
            )}
          </div>
          <div className="flex items-center gap-1">
            <button onClick={startNewChat} className="p-1.5 text-neutral-500 hover:text-white transition-colors" title="New chat">
              <Plus className="w-3.5 h-3.5" />
            </button>
          </div>
        </div>

        {/* Messages */}
        <div className="flex-1 overflow-y-auto px-4 py-4 space-y-4">
          {messages.length === 0 && !isStreaming && (
            <div className="flex flex-col items-center justify-center h-full text-neutral-600">
              <Brain className="w-10 h-10 mb-3 text-neutral-700" />
              <p className="text-sm">Ask Jarvis anything</p>
              <p className="text-xs mt-1 text-neutral-700">Supports text, images, and multi-turn conversation</p>
            </div>
          )}

          {messages.map((msg) => (
            <div key={msg.id} className={cn(
              'flex',
              msg.role === 'user' ? 'justify-end' : 'justify-start',
            )}>
              <div className={cn(
                'max-w-[85%] rounded-xl px-4 py-3',
                msg.role === 'user'
                  ? 'bg-blue-600/20 text-blue-100 rounded-br-sm'
                  : 'bg-neutral-800/60 text-neutral-200 rounded-bl-sm',
              )}>
                {/* User images */}
                {msg.images && msg.images.length > 0 && (
                  <div className="flex flex-wrap gap-2 mb-2">
                    {msg.images.map((img, i) => (
                      <img key={i} src={img} alt="Uploaded" className="max-h-40 rounded-lg object-cover" />
                    ))}
                  </div>
                )}
                {/* Content */}
                {msg.role === 'assistant' ? (
                  <div className="prose prose-sm prose-invert max-w-none
                    prose-pre:bg-neutral-900 prose-pre:border prose-pre:border-neutral-700 prose-pre:rounded-lg
                    prose-code:text-amber-300 prose-code:bg-neutral-900/50 prose-code:px-1 prose-code:py-0.5 prose-code:rounded
                    prose-p:my-1.5 prose-headings:my-2 prose-ul:my-1 prose-li:my-0.5
                    prose-a:text-blue-400 prose-a:no-underline hover:prose-a:underline
                    prose-table:text-xs">
                    <ReactMarkdown remarkPlugins={[remarkGfm]}>
                      {msg.content}
                    </ReactMarkdown>
                  </div>
                ) : (
                  <div className="text-sm whitespace-pre-wrap">{msg.content}</div>
                )}
              </div>
            </div>
          ))}

          {/* Tool activity indicators */}
          {tools.length > 0 && isStreaming && (
            <div className="flex justify-start">
              <div className="bg-neutral-900/60 rounded-lg px-3 py-2 space-y-1">
                {tools.slice(-5).map((tool) => (
                  <div key={tool.id} className="flex items-center gap-2 text-[11px]">
                    {tool.status === 'running' ? (
                      <Loader2 className="w-3 h-3 animate-spin text-amber-400" />
                    ) : (
                      <Wrench className="w-3 h-3 text-green-400" />
                    )}
                    <span className="text-neutral-400 font-mono">{tool.tool}</span>
                    {tool.params && (
                      <span className="text-neutral-600 truncate max-w-[200px]">{tool.params}</span>
                    )}
                    {tool.durationMs !== undefined && (
                      <span className="text-neutral-600">{tool.durationMs}ms</span>
                    )}
                  </div>
                ))}
              </div>
            </div>
          )}

          {/* Error display */}
          {error && (
            <div className="flex justify-center">
              <div className="bg-red-900/20 border border-red-800/30 text-red-400 text-xs px-3 py-2 rounded-lg max-w-md">
                {error}
              </div>
            </div>
          )}

          <div ref={messagesEndRef} />
        </div>

        {/* Image previews */}
        {images.length > 0 && (
          <div className="px-4 pb-2 flex gap-2 flex-wrap">
            {images.map((img, i) => (
              <div key={i} className="relative group">
                <img src={img} alt="Preview" className="h-16 rounded-lg object-cover border border-neutral-700" />
                <button
                  onClick={() => removeImage(i)}
                  className="absolute -top-1.5 -right-1.5 w-4 h-4 bg-neutral-700 rounded-full flex items-center justify-center
                    opacity-0 group-hover:opacity-100 transition-opacity"
                >
                  <X className="w-2.5 h-2.5 text-white" />
                </button>
              </div>
            ))}
          </div>
        )}

        {/* Input area */}
        <div className="border-t border-neutral-800 p-3 bg-neutral-950">
          <div className="flex items-end gap-2">
            <button
              onClick={() => fileInputRef.current?.click()}
              className="p-2 text-neutral-500 hover:text-white transition-colors flex-shrink-0"
              title="Upload image"
            >
              <ImagePlus className="w-4 h-4" />
            </button>
            <input
              ref={fileInputRef}
              type="file"
              accept="image/*"
              multiple
              className="hidden"
              onChange={(e) => handleImageUpload(e.target.files)}
            />

            <textarea
              ref={inputRef}
              value={input}
              onChange={(e) => { setInput(e.target.value); adjustTextareaHeight(); }}
              onKeyDown={handleKeyDown}
              onPaste={handlePaste}
              placeholder="Message Jarvis..."
              rows={1}
              className="flex-1 bg-neutral-900 border border-neutral-800 rounded-lg px-3 py-2 text-sm text-white
                placeholder-neutral-600 resize-none focus:outline-none focus:border-neutral-600 transition-colors
                min-h-[36px] max-h-[200px]"
              disabled={isStreaming}
            />

            {isStreaming ? (
              <button
                onClick={handleStop}
                className="p-2 bg-neutral-700 hover:bg-neutral-600 rounded-lg transition-colors flex-shrink-0"
                title="Stop"
              >
                <div className="w-3.5 h-3.5 bg-white rounded-sm" />
              </button>
            ) : (
              <button
                onClick={sendMessage}
                disabled={!input.trim() && images.length === 0}
                className={cn(
                  'p-2 rounded-lg transition-colors flex-shrink-0',
                  input.trim() || images.length > 0
                    ? 'bg-blue-600 hover:bg-blue-500 text-white'
                    : 'bg-neutral-800 text-neutral-600 cursor-not-allowed',
                )}
                title="Send (Enter)"
              >
                <Send className="w-4 h-4" />
              </button>
            )}
          </div>
          <p className="text-[10px] text-neutral-700 mt-1.5 text-center">
            Shift+Enter for newline · Paste or drag images · Context auto-injected
          </p>
        </div>
      </div>
    </div>
  );
}
