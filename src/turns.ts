import { changed } from "./files";
import { t as text } from "./i18n";
import type { Turn } from "./types";
import { formatCost, formatDuration, formatTokens, totalTokens } from "./usageFormat";

/** "#3", or "Earlier" for activity before the first prompt that was read. */
export function turnLabel(t: Turn): string {
  return t.index === 0 ? text("turn.earlier") : `#${t.index}`;
}

/** Seconds from the prompt to the turn's last line, when both are known. */
export function turnDurationSecs(t: Turn): number | null {
  if (!t.started_at || !t.ended_at) return null;
  const secs = Math.round((Date.parse(t.ended_at) - Date.parse(t.started_at)) / 1000);
  return Number.isFinite(secs) && secs >= 0 ? secs : null;
}

/** Compact figures for one turn, in display order. */
export function turnFacts(t: Turn): string[] {
  const facts: string[] = [];
  const secs = turnDurationSecs(t);
  if (secs !== null) facts.push(formatDuration(secs));
  if (t.usage) {
    facts.push(text("usage.tokens", { tokens: formatTokens(totalTokens(t.usage)) }));
    facts.push(formatCost(t.usage.cost_usd, t.usage.cost_source));
  }
  if (t.tool_calls > 0) {
    facts.push(
      t.tool_errors > 0
        ? text("turn.toolCallsFailed", { n: t.tool_calls, failed: t.tool_errors })
        : text("turn.toolCalls", { n: t.tool_calls }),
    );
  }
  const changedFiles = t.files.filter(changed).length;
  if (changedFiles > 0) facts.push(text("turn.filesChanged", { n: changedFiles }));
  return facts;
}
