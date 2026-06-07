import type { CollectionFileDto } from "../types/catalog";
import { useI18n } from "../i18n/I18nContext";
import { MissingFileRow } from "./MissingFileRow";

type Props = {
  files: CollectionFileDto[];
  onRelink: (fileId: number) => void | Promise<void>;
  onRemove: (fileId: number, title: string) => void;
  busy?: boolean;
  relinkDisabled?: boolean;
  relinkDisabledHint?: string;
  id?: string;
};

export function MissingFilesPanel({
  files,
  onRelink,
  onRemove,
  busy = false,
  relinkDisabled = false,
  relinkDisabledHint,
  id = "missing-files-heading",
}: Props) {
  const { t } = useI18n();
  const missing = files.filter((f) => f.unavailable);
  if (missing.length === 0) return null;

  return (
    <section className="missing-files-panel" aria-labelledby={id}>
      <header className="missing-files-panel-head">
        <h3 id={id} className="missing-files-panel-title">
          {t("catalog.relinkHeading")}
        </h3>
        <p className="missing-files-panel-lead">{t("catalog.relinkHint")}</p>
        <p className="missing-files-panel-count" aria-live="polite">
          {t("catalog.missingFileCount", { count: missing.length })}
        </p>
      </header>
      {relinkDisabled && relinkDisabledHint ? (
        <p className="missing-files-panel-note" role="status">
          {relinkDisabledHint}
        </p>
      ) : null}
      <ul className="missing-files-panel-list">
        {missing.map((f) => (
          <MissingFileRow
            key={f.id}
            fileId={f.id}
            title={f.display_title}
            onRelink={onRelink}
            onRemove={onRemove}
            busy={busy}
            relinkDisabled={relinkDisabled}
            relinkDisabledHint={relinkDisabledHint}
          />
        ))}
      </ul>
    </section>
  );
}
