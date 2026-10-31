import { useCallback, useEffect, useRef, useState } from "react";
import { getLogs } from "../api";

const FOLLOW_MS = 2_000;
const TAIL = 200;

export function LogsPanel({ containerKey }: { containerKey: string }) {
  const [text, setText] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [follow, setFollow] = useState(true);
  const [search, setSearch] = useState("");
  const [copied, setCopied] = useState(false);
  const pre = useRef<HTMLPreElement>(null);

  const load = useCallback(async () => {
    try {
      setText(await getLogs(containerKey, TAIL));
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [containerKey]);

  useEffect(() => {
    // load() awaits the Tauri command before setting state; no synchronous setState here.
    // oxlint-disable-next-line react/set-state-in-effect
    void load();
    if (!follow) return;
    const timer = window.setInterval(() => void load(), FOLLOW_MS);
    return () => window.clearInterval(timer);
  }, [load, follow]);

  useEffect(() => {
    if (follow && pre.current) pre.current.scrollTop = pre.current.scrollHeight;
  }, [follow]);

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
        <pre ref={pre} className="log-view">
          {shown.length === 0 || (shown.length === 1 && shown[0] === "")
            ? "(empty)"
            : shown.join("\n")}
        </pre>
      )}
    </div>
  );
}
