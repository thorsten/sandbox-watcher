import { applyConfirmStop } from "./actions";
import { getSettings, saveSettings } from "./api";

/** "Don't ask again" in the stop dialog: persist it like any other setting. */
export async function disableConfirmStop(): Promise<void> {
  applyConfirmStop(false);
  try {
    const current = await getSettings();
    await saveSettings({ ...current, confirm_stop: false });
  } catch {
    // The in-memory flag already applies for this session.
  }
}
