import { describeError } from "../errors";
import { t } from "../i18n";
import type { UpdateCheck } from "../types";

interface Props {
  check: UpdateCheck;
  onDismiss: () => void;
}

/**
 * One line for a check the user started from the menu: in progress, already
 * current, or failed. A found update is shown by `UpdateBanner` instead.
 */
export function UpdateCheckNotice({ check, onDismiss }: Props) {
  if (check.outcome === "available") return null;
  const tone =
    check.outcome === "failed" ? "is-error" : check.outcome === "up-to-date" ? "is-ok" : "";
  const message =
    check.outcome === "checking"
      ? t("update.checking")
      : check.outcome === "up-to-date"
        ? t("update.upToDate", { version: check.current_version })
        : describeError(check.error);
  return (
    <section className={`update-banner update-check ${tone}`.trim()} aria-live="polite">
      <span>{message}</span>
      {check.outcome !== "checking" && (
        <button type="button" onClick={onDismiss}>
          {t("update.dismiss")}
        </button>
      )}
    </section>
  );
}
