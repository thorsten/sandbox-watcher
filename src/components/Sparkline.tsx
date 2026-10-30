import { useId, useState } from "react";
import { areaPath, linePath, sparkPoints } from "../sparkline";

interface Props {
  title: string;
  values: number[];
  /** Pin the top of the y axis, e.g. 100 for percentages. */
  max?: number;
  format: (value: number) => string;
  /** Label for the latest value, shown beside the title. */
  current: string;
}

const WIDTH = 240;
const HEIGHT = 44;

/** Single-series sparkline: 2px line, soft area, hover value. */
export function Sparkline({ title, values, max, format, current }: Props) {
  const gradientId = useId();
  const [hover, setHover] = useState<number | null>(null);
  const points = sparkPoints(values, WIDTH, HEIGHT, max);
  const hovered = hover !== null ? points[hover] : undefined;
  const hoveredValue = hover !== null ? values[hover] : undefined;

  return (
    <figure className="spark">
      <figcaption>
        <span className="spark-title">{title}</span>
        <span className="spark-value">
          {hoveredValue !== undefined ? format(hoveredValue) : current}
        </span>
      </figcaption>
      {/* Hover is a supplement: the latest value sits in the caption and the label. */}
      {/* oxlint-disable-next-line jsx-a11y/no-noninteractive-element-interactions */}
      <svg
        viewBox={`0 0 ${WIDTH} ${HEIGHT}`}
        width="100%"
        height={HEIGHT}
        preserveAspectRatio="none"
        aria-label={`${title}, latest ${current}`}
        onMouseLeave={() => setHover(null)}
        onMouseMove={(e) => {
          if (points.length < 2) return;
          const rect = e.currentTarget.getBoundingClientRect();
          const ratio = (e.clientX - rect.left) / rect.width;
          setHover(Math.round(Math.min(1, Math.max(0, ratio)) * (points.length - 1)));
        }}
      >
        <defs>
          <linearGradient id={gradientId} x1="0" y1="0" x2="0" y2="1">
            <stop offset="0" stopColor="var(--accent)" stopOpacity="0.28" />
            <stop offset="1" stopColor="var(--accent)" stopOpacity="0.02" />
          </linearGradient>
        </defs>
        {points.length > 1 && (
          <>
            <path d={areaPath(points, HEIGHT)} fill={`url(#${gradientId})`} />
            <path
              d={linePath(points)}
              fill="none"
              stroke="var(--accent)"
              strokeWidth={2}
              vectorEffect="non-scaling-stroke"
              strokeLinejoin="round"
            />
          </>
        )}
        {hovered && (
          <>
            <line
              x1={hovered.x}
              x2={hovered.x}
              y1={0}
              y2={HEIGHT}
              stroke="var(--muted)"
              strokeWidth={1}
              vectorEffect="non-scaling-stroke"
            />
            <circle
              cx={hovered.x}
              cy={hovered.y}
              r={4}
              fill="var(--accent)"
              stroke="var(--card)"
              strokeWidth={2}
            />
          </>
        )}
        {points.length <= 1 && (
          <text x={WIDTH / 2} y={HEIGHT / 2 + 4} textAnchor="middle" className="spark-empty">
            collecting…
          </text>
        )}
      </svg>
    </figure>
  );
}
