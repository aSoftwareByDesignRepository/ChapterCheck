/**
 * Intended `set_paused` value for a Play/Pause control.
 * Uses last known UI paused state so a stale Pause cannot unpause
 * a just-fired sleep timer (mpv toggle would).
 */
export function nextPausedIntent(currentlyPaused: boolean): boolean {
  return !currentlyPaused;
}
