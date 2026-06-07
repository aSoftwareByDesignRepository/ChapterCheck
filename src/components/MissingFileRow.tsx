import { useI18n } from "../i18n/I18nContext";
import { useContextMenu } from "../context/ContextMenuContext";
import { missingFileContextEntries } from "../utils/missingFileMenu";

type Props = {
  fileId: number;
  title: string;
  subtitle?: string | null;
  onRelink: (fileId: number) => void | Promise<void>;
  onRemove: (fileId: number, title: string) => void;
  busy?: boolean;
  relinkDisabled?: boolean;
  relinkDisabledHint?: string;
};

export function MissingFileRow({
  fileId,
  title,
  subtitle,
  onRelink,
  onRemove,
  busy = false,
  relinkDisabled = false,
  relinkDisabledHint,
}: Props) {
  const { t } = useI18n();
  const { openContextMenu } = useContextMenu();

  return (
    <li
      className="missing-file-row"
      onContextMenu={(e) => {
        e.preventDefault();
        e.stopPropagation();
        openContextMenu(
          e,
          missingFileContextEntries(fileId, title, { onRelink, onRemove }, t),
        );
      }}
    >
      <div className="missing-file-row-text">
        <span className="missing-file-row-title">{title}</span>
        {subtitle ? <span className="missing-file-row-sub">{subtitle}</span> : null}
        <span className="missing-file-row-badge">{t("catalog.fileMissing")}</span>
      </div>
      <div className="missing-file-row-actions">
        <button
          type="button"
          className="btn btn-secondary btn-compact"
          disabled={busy || relinkDisabled}
          title={relinkDisabled ? relinkDisabledHint : undefined}
          aria-label={t("catalog.relinkFile")}
          onClick={() => void onRelink(fileId)}
        >
          {busy ? t("catalog.busyRelink") : t("catalog.relinkFile")}
        </button>
        <button
          type="button"
          className="btn btn-ghost btn-compact"
          disabled={busy}
          aria-label={t("catalog.removeFromLibrary")}
          onClick={() => onRemove(fileId, title)}
        >
          {t("catalog.removeFromLibrary")}
        </button>
      </div>
    </li>
  );
}
