import { useContextMenu, type ContextMenuEntry } from "../context/ContextMenuContext";
import { useI18n } from "../i18n/I18nContext";
import type { LibraryRootDto } from "../types/catalog";

type Props = {
  libraryRoots: LibraryRootDto[];
  onLinkFolder: () => void;
  onScanRoot: (rootId: number) => void;
  onRemoveRoot: (root: LibraryRootDto) => void;
  onExportDb: () => void;
};

export function LibraryView({
  libraryRoots,
  onLinkFolder,
  onScanRoot,
  onRemoveRoot,
  onExportDb,
}: Props) {
  const { t } = useI18n();
  const { openContextMenu } = useContextMenu();

  return (
    <div className="view-panel library-view">
      <header className="library-view-head">
        <div className="library-view-head-top">
          <div className="library-view-head-text">
            <h1 className="view-title">{t("sidebar.manageLibrary")}</h1>
            <p className="view-lead">{t("library.intro")}</p>
          </div>
          <button
            type="button"
            className="btn btn-primary library-view-link-btn"
            onClick={onLinkFolder}
          >
            {t("library.linkFolder")}
          </button>
        </div>
      </header>

      {libraryRoots.length > 0 ? (
        <ul className="library-root-list library-root-list--page">
          {libraryRoots.map((root) => (
            <li
              key={root.id}
              className="library-root-item"
              onContextMenu={(e) => {
                const items: ContextMenuEntry[] = [
                  {
                    id: "scan",
                    label: t("library.scan"),
                    onClick: () => onScanRoot(root.id),
                  },
                  { type: "separator" },
                  {
                    id: "remove",
                    label: t("library.remove"),
                    danger: true,
                    onClick: () => onRemoveRoot(root),
                  },
                ];
                openContextMenu(e, items);
              }}
            >
              <div className="library-root-toolbar">
                <span className="library-root-label">{root.label}</span>
                <div className="library-root-toolbar-actions">
                  <span className="library-root-kind-badge">
                    {root.content_kind === "music"
                      ? t("library.kind.music")
                      : root.content_kind === "mixed"
                        ? t("library.kind.mixed")
                        : t("library.kind.audiobook")}
                  </span>
                  {!root.is_available ? (
                    <span className="library-root-status-badge">{t("library.unavailable")}</span>
                  ) : null}
                  <button
                    type="button"
                    className="btn btn-secondary btn-compact"
                    onClick={() => onScanRoot(root.id)}
                  >
                    {t("library.scan")}
                  </button>
                  <button
                    type="button"
                    className="btn btn-ghost btn-compact"
                    onClick={() => onRemoveRoot(root)}
                  >
                    {t("library.remove")}
                  </button>
                </div>
              </div>
              <p className="library-root-path">{root.path}</p>
              <p className="library-root-meta">
                {t("library.collections", { count: root.collection_count })}
              </p>
            </li>
          ))}
        </ul>
      ) : (
        <div className="view-empty view-empty--actions">
          <p className="view-empty-title">{t("library.emptyTitle")}</p>
          <p className="view-empty-body">{t("library.emptyBody")}</p>
        </div>
      )}

      <footer className="library-view-footer">
        <button type="button" className="btn btn-ghost btn-compact" onClick={onExportDb}>
          {t("library.exportDb")}
        </button>
        <p className="hint library-view-hint">{t("library.exportHint")}</p>
      </footer>
    </div>
  );
}
