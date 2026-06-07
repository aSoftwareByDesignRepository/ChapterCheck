import { useI18n } from "../i18n/I18nContext";

type Props = {
  linkedFolderCount: number;
  onLinkFolder: () => void;
  onManageLibrary: () => void;
  onOpenFolder: () => void;
  onOpenFile: () => void;
};

export function LibrarySidebar({
  linkedFolderCount,
  onLinkFolder,
  onManageLibrary,
  onOpenFolder,
  onOpenFile,
}: Props) {
  const { t } = useI18n();
  const hasLibrary = linkedFolderCount > 0;

  if (hasLibrary) {
    return (
      <div className="sidebar-library-block sidebar-library-block--compact">
        <div className="sidebar-compact-actions">
          <button type="button" className="btn btn-ghost btn-compact sidebar-compact-btn" onClick={onLinkFolder}>
            {t("sidebar.addFolder")}
          </button>
          <button type="button" className="btn btn-ghost btn-compact sidebar-compact-btn" onClick={onManageLibrary}>
            {t("sidebar.manageLibrary")}
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="sidebar-library-block">
      <section className="sidebar-section sidebar-section--library" aria-labelledby="sidebar-library-heading">
        <h2 className="sidebar-heading sidebar-heading--compact" id="sidebar-library-heading">
          {t("sidebar.getStarted")}
        </h2>
        <p className="sidebar-section-lead">{t("sidebar.libraryLead")}</p>
        <button type="button" className="btn btn-primary btn-sidebar" onClick={onLinkFolder}>
          {t("library.linkFolder")}
        </button>
      </section>

      <section className="sidebar-section sidebar-section--quick" aria-labelledby="sidebar-quick-heading">
        <h2 className="sidebar-heading sidebar-heading--compact" id="sidebar-quick-heading">
          {t("sidebar.quickListen")}
        </h2>
        <div className="sidebar-compact-actions sidebar-compact-actions--stack">
          <button type="button" className="btn btn-ghost btn-sidebar" onClick={onOpenFolder}>
            {t("sidebar.openFolder")}
          </button>
          <button type="button" className="btn btn-ghost btn-sidebar" onClick={onOpenFile}>
            {t("sidebar.openFile")}
          </button>
        </div>
      </section>
    </div>
  );
}
