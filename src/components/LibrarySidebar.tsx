import { useI18n } from "../i18n/I18nContext";

type Props = {
  linkedFolderCount: number;
  onLinkFolder: () => void;
  onManageLibrary: () => void;
};

export function LibrarySidebar({ linkedFolderCount, onLinkFolder, onManageLibrary }: Props) {
  const { t } = useI18n();

  if (linkedFolderCount === 0) {
    return null;
  }

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
