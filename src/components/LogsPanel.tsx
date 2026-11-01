import { useCallback, useEffect, useRef, useState } from "react";
import { getLogs } from "../api";
import { usePolling } from "../usePolling";

export const FOLLOW_MS = 2_000;
const TAIL = 200;
/** Distance from the bottom, in pixels, that still counts as "at the end". */
const BOTTOM_SLACK = 4;

/**
 * Follow semantics:
 * - Follow on: the log is re-read every FOLLOW_MS after the previous read finished, and the view
 *   jumps to the end whenever new text arrives or Follow is switched on.
 * - Scrolling up by hand while Follow is on switches Follow off, so the position the user chose
 *   is never taken away. Scrolling back to the end does not switch it on again; the checkbox does.
 * - Follow off: the log is read once (on container change or when Follow was just switched off)
 *   and the scroll position is left alone.
 */
export function LogsPanel({ containerKey }: { containerKey: string }) {
  const [follow, setFollow] = useState(true);
  const [search, setSearch] = useState("");
  const [copied, setCopied] = useState(false);
  const pre = useRef<HTMLPreElement>(null);

  const load = useCallback(() => getLogs(containerKey, TAIL), [containerKey]);
  const { data: text, error } = usePolling(load, follow ? FOLLOW_MS : null);

  useEffect(() => {
    if (follow && text !== null && pre.current) pre.current.scrollTop = pre.current.scrollHeight;
  }, [follow, text]);

  const onScroll = () => {
    const el = pre.current;
    if (!el || !follow) return;
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight <= BOTTOM_SLACK;
    if (!atBottom) setFollow(false);
  };

  const lines = (text ?? "").split("\n");
  const needle = search.trim().toLowerCase();
  const shown = needle ? lines.filter((l) => l.toLowerCase().includes(needle)) : lines;

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(shown.join("\n"));
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch {
      setCopied(false);
    }
  };

  return (
    <div className="logs">
      <div className="network-toolbar">
        <label className="check">
          <input type="checkbox" checked={follow} onChange={(e) => setFollow(e.target.checked)} />
          Follow
        </label>
        <input
          type="search"
          placeholder="Filter lines"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          aria-label="Filter log lines"
        />
        <button type="button" onClick={() => void copy()}>
          {copied ? "Copied" : "Copy"}
        </button>
        <span className="muted">
          last {TAIL} lines{needle ? `, ${shown.length} match` : ""}
        </span>
      </div>
      {error && <p className="network-note warn">{error}</p>}
      {text === null && !error && <p className="muted">Reading container log…</p>}
      {text !== null && (
        <pre ref={pre} className="log-view" onScroll={onScroll}>
          {shown.length === 0 || (shown.length === 1 && shown[0] === "")
            ? "(empty)"
            : shown.join("\n")}
        </pre>
      )}
    </div>
  );
}
