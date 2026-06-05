"use client";

import { Conversations } from "./Conversations";

export function CodexConversations() {
  return (
    <Conversations
      fixedViewMode="codex"
      storageScope="codex-conversations"
      hideViewTabs
    />
  );
}
