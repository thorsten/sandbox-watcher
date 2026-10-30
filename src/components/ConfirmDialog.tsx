import { useEffect, useRef, useState } from "react";

interface Props {
  title: string;
  lines: string[];
  confirmLabel: string;
  busy?: boolean;
  onConfirm: (dontAskAgain: boolean) => void;
  onCancel: () => void;
}

export function ConfirmDialog({ title, lines, confirmLabel, busy, onConfirm, onCancel }: Props) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [dontAsk, setDontAsk] = useState(false);

  useEffect(() => {
    const el = dialog.current;
    if (el && !el.open) el.showModal();
    return () => el?.close();
  }, []);

  return (
    <dialog ref={dialog} className="confirm" onCancel={onCancel} aria-labelledby="confirm-title">
      <h3 id="confirm-title">{title}</h3>
      <ul>
        {lines.map((line) => (
          <li key={line}>{line}</li>
        ))}
      </ul>
      <label className="check">
        <input type="checkbox" checked={dontAsk} onChange={(e) => setDontAsk(e.target.checked)} />
        Don't ask again
      </label>
      <div className="confirm-actions">
        <button type="button" onClick={onCancel} disabled={busy}>
          Cancel
        </button>
        <button type="button" className="danger" onClick={() => onConfirm(dontAsk)} disabled={busy}>
          {busy ? "Stopping…" : confirmLabel}
        </button>
      </div>
    </dialog>
  );
}
