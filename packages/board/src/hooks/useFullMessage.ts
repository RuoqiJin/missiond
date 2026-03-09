import { useState, useEffect } from 'react';
import type { FullMessage } from '../components/timeline/types';

// Module-level cache: survives component unmount/remount, shared across all consumers
const messageCache = new Map<number, FullMessage>();
const inflightRequests = new Map<number, Promise<FullMessage | null>>();

function parseMessage(data: Record<string, unknown>): FullMessage | null {
  if (!data?.content) return null;
  let blocks: FullMessage['contentBlocks'] = null;
  let imageCount = 0;
  if (data.raw_content) {
    try {
      blocks = JSON.parse(data.raw_content as string);
      if (Array.isArray(blocks)) {
        for (const block of blocks) {
          if (block?.type === 'image') {
            imageCount++;
            if (block.source?.data) block.source.data = '[stripped]';
          }
        }
      }
    } catch { /* ignore */ }
  }
  return {
    content: data.content as string,
    contentBlocks: blocks,
    imageCount,
    translation: (data.translation as string) || null,
  };
}

/** Fetch full message content + raw_content blocks with module-level caching + inflight dedup. */
export function useFullMessage(messageId: number | undefined, enabled: boolean): FullMessage | null {
  const [msg, setMsg] = useState<FullMessage | null>(() =>
    messageId ? messageCache.get(messageId) ?? null : null,
  );

  useEffect(() => {
    if (!messageId || !enabled) return;

    // Cache hit — use immediately, skip fetch
    const cached = messageCache.get(messageId);
    if (cached) { setMsg(cached); return; }

    let isMounted = true;

    // Reuse inflight request for same messageId (dedup concurrent fetches)
    if (!inflightRequests.has(messageId)) {
      const promise = fetch(`/api/system/conversation-message?message_id=${messageId}`)
        .then(r => r.json())
        .then(data => {
          const result = parseMessage(data);
          if (result) messageCache.set(messageId, result);
          return result;
        })
        .catch(() => null)
        .finally(() => { inflightRequests.delete(messageId); });
      inflightRequests.set(messageId, promise);
    }

    inflightRequests.get(messageId)!.then(result => {
      if (isMounted && result) setMsg(result);
    });

    return () => { isMounted = false; };
  }, [messageId, enabled]);

  return msg;
}
