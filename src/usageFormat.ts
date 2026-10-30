import type { Usage } from "./types";

export function formatTokens(n: number): string {
  if (n < 1000) return String(n);
  if (n < 1_000_000) return `${(n / 1000).toFixed(n < 10_000 ? 1 : 0)}k`;
  return `${(n / 1_000_000).toFixed(2)}M`;
}

export function formatCost(
  usd: number | null,
  source: Usage["cost_source"] | "mixed" = "estimated",
): string {
  if (usd === null) return "no price";
  const amount = usd < 0.01 ? `$${usd.toFixed(4)}` : `$${usd.toFixed(2)}`;
  return source === "estimated" ? `≈${amount}` : amount;
}

export function formatDuration(secs: number): string {
  if (secs < 60) return `${secs}s`;
  const m = Math.floor(secs / 60);
  if (m < 60) return `${m}m`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h ${m % 60}m`;
  const d = Math.floor(h / 24);
  return `${d}d ${h % 24}h`;
}

export function totalTokens(u: Usage): number {
  return u.input_tokens + u.output_tokens + u.cache_read_tokens + u.cache_write_tokens;
}
