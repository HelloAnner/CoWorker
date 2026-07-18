import { describe, expect, it } from "vitest";
import type { DictKey } from "../i18n/en";
import type { ActorMessage } from "../tauri";
import {
  actorMessagesToTimelineMessages,
  isTimelineNearBottom,
  normalizeActorMessageAuthorKind,
} from "./ActorConversationParts";

const t = (key: DictKey, vars?: Record<string, string | number>) => vars?.tool ? `${key}:${vars.tool}` : key;

function message(overrides: Partial<ActorMessage> = {}): ActorMessage {
  return {
    actor_id: "claude",
    author_kind: "assistant",
    content: "Hello",
    conversation_id: "conversation-1",
    created_at: "2026-07-13T10:00:00Z",
    id: "message-1",
    metadata: null,
    ...overrides,
  };
}

describe("actor conversation boundaries", () => {
  it("only follows new messages while the timeline remains near the bottom", () => {
    expect(isTimelineNearBottom({ scrollHeight: 1_000, scrollTop: 628, clientHeight: 300 })).toBe(true);
    expect(isTimelineNearBottom({ scrollHeight: 1_000, scrollTop: 300, clientHeight: 300 })).toBe(false);
  });

  it("normalizes actor authors into the shared message sides", () => {
    expect(normalizeActorMessageAuthorKind("local", "local")).toBe("local");
    expect(normalizeActorMessageAuthorKind("coworker", "local")).toBe("coworker");
    expect(normalizeActorMessageAuthorKind("assistant", "claude")).toBe("codex");
    expect(normalizeActorMessageAuthorKind("claude", "claude")).toBe("codex");
  });

  it("normalizes actor attachments into shared session attachments", () => {
    const [converted] = actorMessagesToTimelineMessages([message({
      author_kind: "coworker",
      metadata: {
        attachments: [{
          filename: "brief.pdf",
          media_type: "application/pdf",
          path: "D:\\attachments\\brief.pdf",
          size: 128,
        }],
      },
    })], "claude", t);

    expect(converted.author_kind).toBe("coworker");
    expect(converted.attachments).toEqual([expect.objectContaining({
      filename: "brief.pdf",
      path: "D:\\attachments\\brief.pdf",
      downloadable: true,
    })]);
  });

  it("normalizes actor tool exchanges into shared call and result messages", () => {
    const converted = actorMessagesToTimelineMessages([message({
      author_kind: "tool",
      content: "[tool] Read",
      id: "tool-message",
      metadata: {
        kind: "tool",
        tool_use_id: "toolu_1",
        tool_name: "Read",
        input: { file_path: "D:\\workspace\\README.md" },
        output: "file contents",
        is_error: false,
        result_id: "tool-result",
      },
    })], "claude", t);

    expect(converted).toHaveLength(2);
    expect(converted[0]).toMatchObject({ kind: "tool_call", item_id: "toolu_1", streaming: false, tool_name: "Read" });
    expect(converted[0].text).toContain("README.md");
    expect(converted[1]).toMatchObject({ id: "tool-result", kind: "tool_result", item_id: "toolu_1", is_error: false });
    expect(converted[1].text).toContain("file contents");
  });
});
