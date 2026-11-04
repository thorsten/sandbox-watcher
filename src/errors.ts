/**
 * Text for what a Tauri command rejected with. A `CommandError` is
 * translated through the dictionaries, with its fields as parameters; an
 * `Error` or anything else is shown as it is. Every place that shows a
 * caught rejection goes through `describeError`.
 */
import { type PlainKey, t } from "./i18n";
import type { CommandError } from "./types";

type Code = CommandError["code"];

/** The dictionary key per code; `tsc` fails when a code has no entry. */
const KEYS = {
  "local-only": "error.localOnly",
  "overview-not-ready": "error.overviewNotReady",
  "unknown-container": "error.unknownContainer",
  "container-not-found": "error.containerNotFound",
  "not-a-mount": "error.notAMount",
  "no-project-directory": "error.noProjectDirectory",
  "path-outside-project": "error.pathOutsideProject",
  "no-editor": "error.noEditor",
  "no-terminal": "error.noTerminal",
  "open-failed": "error.openFailed",
  "no-global-config": "error.noGlobalConfig",
  "no-sandbox-directory": "error.noSandboxDirectory",
  "preview-required": "error.previewRequired",
  "config-changed": "error.configChanged",
  "config-refused": "error.configRefused",
  "config-write-failed": "error.configWriteFailed",
  "invalid-alias": "error.invalidAlias",
  "host-not-connected": "error.hostNotConnected",
  "no-config-directory": "error.noConfigDirectory",
  "settings-write-failed": "error.settingsWriteFailed",
  "update-check-failed": "error.updateCheckFailed",
  "no-update-pending": "error.noUpdatePending",
  "update-install-failed": "error.updateInstallFailed",
  "runtime-not-detected": "error.runtimeNotDetected",
  "runtime-unavailable": "error.runtimeUnavailable",
  "permission-denied": "error.permissionDenied",
  unsupported: "error.unsupported",
  "runtime-response": "error.runtimeResponse",
  other: "error.other",
} as const satisfies Record<Code, PlainKey>;

/** Whether `e` is a `CommandError`, optionally one with the given code. */
export function isCommandError(e: unknown, code?: Code): e is CommandError {
  if (typeof e !== "object" || e === null || !("code" in e)) return false;
  const found = (e as { code: unknown }).code;
  if (typeof found !== "string" || !(found in KEYS)) return false;
  return code === undefined || found === code;
}

/** The sentence for a caught rejection in the current language. */
export function describeError(e: unknown): string {
  if (isCommandError(e)) {
    const { code, ...params } = e;
    return t(KEYS[code], params);
  }
  return e instanceof Error ? e.message : String(e);
}
