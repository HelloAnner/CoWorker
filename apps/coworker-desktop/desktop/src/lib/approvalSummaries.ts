/**
 * Approval summary and risk-level helpers for the Desktop approval panel.
 *
 * `summarizeApproval` extracts a one-line human-readable summary from a tool
 * name + input pair so the user can see *what* is about to happen without
 * expanding the raw JSON.
 *
 * `riskLevel` classifies the request as low / medium / high so the panel can
 * adjust its visual emphasis.
 */

export type RiskLevel = "low" | "medium" | "high";

/** Maximum characters shown for a summary before truncation. */
const SUMMARY_MAX = 120;

// ── Summary ────────────────────────────────────────────────────────────────

/**
 * Return a one-line summary string for the given approval request.
 * Falls back to the tool name when no specific extractor matches.
 */
export function summarizeApproval(toolName: string, input: unknown): string {
  if (!input || typeof input !== "object") return toolName;

  const obj = input as Record<string, unknown>;

  switch (toolName) {
    case "Bash": {
      const cmd = String(obj.command ?? "");
      return cmd ? `Run: ${truncate(cmd, SUMMARY_MAX - 5)}` : toolName;
    }
    case "Edit":
    case "Write": {
      const path = String(obj.file_path ?? obj.path ?? "");
      return path ? `${toolName} ${truncate(path, SUMMARY_MAX - 6)}` : toolName;
    }
    case "Read": {
      const path = String(obj.file_path ?? obj.path ?? "");
      return path ? `Read ${truncate(path, SUMMARY_MAX - 5)}` : toolName;
    }
    case "WebFetch": {
      const url = String(obj.url ?? "");
      return url ? `Fetch ${truncate(url, SUMMARY_MAX - 6)}` : toolName;
    }
    case "WebSearch": {
      const query = String(obj.query ?? "");
      return query ? `Search: ${truncate(query, SUMMARY_MAX - 8)}` : toolName;
    }
    case "commandExecution": {
      // Codex approval method — params typically contain { command: "..." }
      const cmd = String(obj.command ?? "");
      return cmd ? `Run: ${truncate(cmd, SUMMARY_MAX - 5)}` : toolName;
    }
    case "fileChange": {
      const path = String(obj.path ?? obj.file_path ?? "");
      return path ? `Change ${truncate(path, SUMMARY_MAX - 7)}` : toolName;
    }
    default: {
      // Generic: try to find a plausible primary field
      for (const key of ["command", "file_path", "path", "url", "query", "content"]) {
        const value = String(obj[key] ?? "");
        if (value) return `${toolName}: ${truncate(value, SUMMARY_MAX - toolName.length - 2)}`;
      }
      return toolName;
    }
  }
}

// ── Risk level ─────────────────────────────────────────────────────────────

/** Patterns that indicate elevated risk in a Bash command. */
const HIGH_RISK_PATTERNS = /\b(rm\s+-r|sudo|chmod|chown|mkfs|dd\s+if=|>\s*\/dev\/|curl\s+.*\|\s*sh|wget\s+.*\|\s*sh)\b/i;
const MEDIUM_RISK_PATTERNS = /\b(pip\s+install|npm\s+install|yarn\s+add|cargo\s+install|apt\b|yum\b|brew\s+install)\b/i;

/**
 * Classify the risk of an approval request.
 *
 * - **high**: destructive shell commands, privilege escalation
 * - **medium**: package installs, network fetches with side effects
 * - **low**: everything else (reads, edits, searches)
 */
export function riskLevel(toolName: string, input: unknown): RiskLevel {
  if (!input || typeof input !== "object") return "low";
  const obj = input as Record<string, unknown>;

  // Bash / commandExecution — inspect the command string
  if (toolName === "Bash" || toolName === "commandExecution") {
    const cmd = String(obj.command ?? "");
    if (HIGH_RISK_PATTERNS.test(cmd)) return "high";
    if (MEDIUM_RISK_PATTERNS.test(cmd)) return "medium";
    return "medium"; // running arbitrary commands is inherently medium+
  }

  // Write / Edit / fileChange — writing is medium
  if (toolName === "Write" || toolName === "Edit" || toolName === "fileChange") {
    return "medium";
  }

  // Everything else is low risk
  return "low";
}

// ── Helpers ────────────────────────────────────────────────────────────────

function truncate(text: string, max: number): string {
  if (text.length <= max) return text;
  return text.slice(0, max - 1) + "…";
}
