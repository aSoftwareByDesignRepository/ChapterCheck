import { useRef } from "react";
import { SLEEP_PRESET_MINUTES } from "../utils/sleepDisplay";
import { useI18n } from "../i18n/I18nContext";

type Props = {
  remainingLabel: string | null;
  stopAfterTrack: boolean;
  presetsDisabled?: boolean;
  onPickMinutes: (minutes: number) => void | Promise<void>;
  onTurnOff: () => void | Promise<void>;
  onStopAfterChange: (enabled: boolean) => void;
};

export function SleepTimerSheet({
  remainingLabel,
  stopAfterTrack,
  presetsDisabled = false,
  onPickMinutes,
  onTurnOff,
  onStopAfterChange,
}: Props) {
  const { t } = useI18n();
  const pickInFlight = useRef(false);

  return (
    <div className="sleep-card sleep-card--modal">
      <p className="sleep-lead" id="sleep-lead">
        {t("sleep.lead")}
      </p>
      <div className="sleep-remaining-block" role="status" aria-live="polite" aria-atomic="true">
        <span className="field-label" id="sleep-status-label">
          {t("sleep.status")}
        </span>
        {remainingLabel ? (
          <p className="sleep-remaining" aria-labelledby="sleep-status-label">
            {remainingLabel}
          </p>
        ) : (
          <p className="sleep-remaining sleep-remaining--off" aria-labelledby="sleep-status-label">
            {t("sleep.off")}
          </p>
        )}
      </div>
      <div className="sleep-presets" role="group" aria-label={t("sleep.minutesLabel")}>
        {SLEEP_PRESET_MINUTES.map((mins) => (
          <button
            key={mins}
            type="button"
            className="btn btn-secondary sleep-preset"
            disabled={presetsDisabled}
            aria-label={t(`sleep.preset.${mins}`)}
            onClick={() => {
              if (presetsDisabled || pickInFlight.current) return;
              pickInFlight.current = true;
              void Promise.resolve(onPickMinutes(mins)).finally(() => {
                pickInFlight.current = false;
              });
            }}
          >
            {t(`sleep.presetShort.${mins}`)}
          </button>
        ))}
      </div>
      <button
        className="btn btn-ghost sleep-turn-off"
        type="button"
        disabled={remainingLabel == null}
        onClick={() => void onTurnOff()}
      >
        {t("sleep.turnOff")}
      </button>
      <label className="prefs-row sleep-stop-row" htmlFor="sleep-stop-after">
        <input
          id="sleep-stop-after"
          type="checkbox"
          checked={stopAfterTrack}
          onChange={(e) => onStopAfterChange(e.target.checked)}
        />
        <span>{t("sleep.stopAfter")}</span>
      </label>
      <p className="hint">{t("sleep.hint")}</p>
    </div>
  );
}
