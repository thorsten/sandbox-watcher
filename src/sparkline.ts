/** Pure helpers for the inline SVG sparklines. */

export interface SparkPoint {
  x: number;
  y: number;
}

/**
 * Map values onto an SVG viewport. The y axis always starts at zero so a
 * flat 2% line never looks like a mountain range; `max` can be pinned, for
 * example to 100 for percentages.
 */
export function sparkPoints(
  values: number[],
  width: number,
  height: number,
  max?: number,
): SparkPoint[] {
  if (values.length === 0) return [];
  const top = Math.max(max ?? 0, ...values, 1e-9);
  const stepX = values.length > 1 ? width / (values.length - 1) : 0;
  return values.map((v, i) => ({
    x: values.length > 1 ? i * stepX : width,
    y: height - (Math.max(0, v) / top) * height,
  }));
}

export function linePath(points: SparkPoint[]): string {
  return points
    .map((p, i) => `${i === 0 ? "M" : "L"}${p.x.toFixed(1)},${p.y.toFixed(1)}`)
    .join(" ");
}

export function areaPath(points: SparkPoint[], height: number): string {
  if (points.length === 0) return "";
  const first = points[0];
  const last = points[points.length - 1];
  if (!first || !last) return "";
  return `${linePath(points)} L${last.x.toFixed(1)},${height} L${first.x.toFixed(1)},${height} Z`;
}

export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return "–";
  const units = ["B", "KiB", "MiB", "GiB", "TiB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit++;
  }
  const digits = unit === 0 ? 0 : value >= 100 ? 0 : value >= 10 ? 1 : 2;
  return `${value.toFixed(digits)} ${units[unit]}`;
}

export function formatPercent(value: number): string {
  return `${value >= 10 ? value.toFixed(0) : value.toFixed(1)}%`;
}
