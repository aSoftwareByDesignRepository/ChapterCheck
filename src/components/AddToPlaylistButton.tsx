import { useAddToPlaylist, type AddToPlaylistTarget } from "../context/AddToPlaylistContext";
import { useI18n } from "../i18n/I18nContext";

type Props = {
  target: AddToPlaylistTarget;
  className?: string;
  compact?: boolean;
};

export function AddToPlaylistButton({ target, className, compact }: Props) {
  const { t } = useI18n();
  const { openAddToPlaylist } = useAddToPlaylist();

  return (
    <button
      type="button"
      className={className ?? (compact ? "track-action" : "btn btn-secondary")}
      aria-label={t("catalog.addToPlaylist")}
      title={t("catalog.addToPlaylist")}
      onClick={() => openAddToPlaylist(target)}
    >
      {compact ? (
        <span className="track-action-icon" aria-hidden="true">
          ♫+
        </span>
      ) : (
        t("catalog.addToPlaylist")
      )}
    </button>
  );
}
