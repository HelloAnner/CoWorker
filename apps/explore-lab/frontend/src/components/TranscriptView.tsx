import type { TranscriptMessage, TranscriptToolCall } from '../api/types';

interface PairedToolCall {
  call: TranscriptToolCall;
  result?: TranscriptMessage;
}

interface TranscriptEntry {
  message: TranscriptMessage;
  messageIndex: number;
  toolCalls: PairedToolCall[];
}

function renderContent(content: TranscriptMessage['content']): string {
  if (typeof content === 'string') return content;
  try {
    return JSON.stringify(content, null, 2);
  } catch {
    return String(content);
  }
}

function parseJsonMaybe(value: unknown): unknown {
  if (typeof value !== 'string') return value;
  try {
    return JSON.parse(value);
  } catch {
    return value;
  }
}

function renderJson(value: unknown): string {
  try {
    return JSON.stringify(parseJsonMaybe(value), null, 2);
  } catch {
    return String(value);
  }
}

function toolName(call: TranscriptToolCall): string {
  return call.function?.name || call.name || '(unknown tool)';
}

function toolArguments(call: TranscriptToolCall): unknown {
  if (call.function && 'arguments' in call.function) return call.function.arguments;
  return call.arguments;
}

function toolCallId(call: TranscriptToolCall): string | undefined {
  return call.id;
}

function buildTranscriptEntries(messages: TranscriptMessage[]): TranscriptEntry[] {
  const resultsByCallId = new Map<string, { message: TranscriptMessage; index: number }>();
  messages.forEach((message, index) => {
    if (message.role === 'tool' && message.tool_call_id && !resultsByCallId.has(message.tool_call_id)) {
      resultsByCallId.set(message.tool_call_id, { message, index });
    }
  });

  const pairedResultIndexes = new Set<number>();
  messages.forEach(message => {
    for (const call of message.tool_calls ?? []) {
      const id = toolCallId(call);
      const match = id ? resultsByCallId.get(id) : undefined;
      if (match) pairedResultIndexes.add(match.index);
    }
  });

  const entries: TranscriptEntry[] = [];

  messages.forEach((message, messageIndex) => {
    const calls = message.tool_calls ?? [];
    if (calls.length > 0) {
      const pairedCalls = calls.map(call => {
        const id = toolCallId(call);
        const match = id ? resultsByCallId.get(id) : undefined;
        return { call, result: match?.message };
      });
      entries.push({ message, messageIndex, toolCalls: pairedCalls });
      return;
    }

    if (message.role === 'tool' && pairedResultIndexes.has(messageIndex)) return;
    entries.push({ message, messageIndex, toolCalls: [] });
  });

  return entries;
}

function ToolCallDetails({ calls }: { calls: PairedToolCall[] }) {
  if (calls.length === 0) return null;
  return (
    <div className="toolcall-list">
      {calls.map(({ call, result }, index) => {
        const id = toolCallId(call);
        const resultText = result ? renderContent(result.content) : '';
        return (
          <details key={id || index} className="toolcall-detail">
            <summary>
              <span className="toolcall-name">{toolName(call)}</span>
              <span className={`toolcall-state ${result ? 'is-done' : 'is-pending'}`}>
                {result ? '已完成' : '等待结果'}
              </span>
              {id && <span className="toolcall-id">{id}</span>}
            </summary>
            <div className="toolcall-section">
              <span className="toolcall-section-title">参数</span>
              <pre className="toolcall-json">{renderJson(toolArguments(call))}</pre>
            </div>
            <div className="toolcall-section">
              <span className="toolcall-section-title">结果</span>
              <pre className="toolcall-json">{result ? resultText || '(空结果)' : '(尚未返回)'}</pre>
            </div>
          </details>
        );
      })}
    </div>
  );
}

export function TranscriptView({ messages }: { messages: TranscriptMessage[] }) {
  return (
    <div className="transcript">
      {buildTranscriptEntries(messages).map(({ message, messageIndex, toolCalls }) => {
        const hasContent = renderContent(message.content).trim().length > 0;
        return (
          <div key={messageIndex} className={`transcript-row role-${message.role}`}>
            <span className="transcript-role">{message.role}</span>
            <div className="transcript-body">
              {message.tool_call_id && <span className="transcript-tool-result">tool_call_id: {message.tool_call_id}</span>}
              {hasContent && <span className="transcript-content">{renderContent(message.content)}</span>}
              {toolCalls.length > 0 && (
                <>
                  <span className="transcript-toolcalls">{toolCalls.length} 个工具调用</span>
                  <ToolCallDetails calls={toolCalls} />
                </>
              )}
            </div>
          </div>
        );
      })}
    </div>
  );
}
