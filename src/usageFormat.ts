import { formatNumber, t } from "./i18n";
import type { Usage } from "./types";

/** Compact token count: 999, 1.5k, 64k, 2.35M, in the language's number format. */
export function formatTokens(n: number): string {
  if (n < 1000) return formatNumber(n);
  if (n < 1_000_000) {
    const digits = n < 10_000 ? 1 : 0;
    return `${formatNumber(n / 1000, { minimumFractionDigits: digits, maximumFractionDigits: digits })}k`;
  }
  return `${formatNumber(n / 1_000_000, { minimumFractionDigits: 2, maximumFractionDigits: 2 })}M`;
}

export function formatCost(
  usd: number | null,
  source: Usage["cost_source"] | "mixed" = "estimated",
): string {
  if (usd === null) return t("cost.noPrice");
  const digits = usd < 0.01 ? 4 : 2;
  const amount = formatNumber(usd, {
    style: "currency",
    currency: "USD",
    minimumFractionDigits: digits,
    maximumFractionDigits: digits,
  });
  return source === "estimated" ? `≈${amount}` : amount;
}

export function formatDuration(secs: number): string {
  if (secs < 60) return t("duration.seconds", { n: secs });
  const m = Math.floor(secs / 60);
  if (m < 60) return t("duration.minutes", { n: m });
  const h = Math.floor(m / 60);
  if (h < 24) return t("duration.hoursMinutes", { h, m: m % 60 });
  const d = Math.floor(h / 24);
  return t("duration.daysHours", { d, h: h % 24 });
}

export function totalTokens(u: Usage): number {
  return u.input_tokens + u.output_tokens + u.cache_read_tokens + u.cache_write_tokens;
}
