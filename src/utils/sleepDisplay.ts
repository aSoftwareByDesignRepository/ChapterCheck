export const SLEEP_PRESET_MINUTES = [15, 30, 45, 60, 90] as const;

export type SleepPresetMinutes = (typeof SLEEP_PRESET_MINUTES)[number];

/** Remaining time for the sleep chip / modal. `null` if the timer is off. */
export function formatSleepRemaining(
  deadlineMs: number | null | undefined,
  nowMs: number,
): string | null {
  if (deadlineMs == null || !Number.isFinite(deadlineMs) || deadlineMs <= 0) {
    return null;
  }
  const sec = Math.max(0, Math.ceil((deadlineMs - nowMs) / 1000));
  const h = Math.floor(sec / 3600);
  const m = Math.floor((sec % 3600) / 60);
  const s = sec % 60;
  if (h > 0) return `${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
  return `${m}:${String(s).padStart(2, "0")}`;
}

export function clampSleepMinutes(minutes: number): number | null {
  if (!Number.isFinite(minutes) || minutes < 1 || minutes > 180) return null;
  return Math.floor(minutes);
}
