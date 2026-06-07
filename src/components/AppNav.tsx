import { useI18n } from "../i18n/I18nContext";
import type { AppView } from "../types/catalog";
import { IconBooks, IconHome, IconList, IconMusic, IconNowPlaying } from "./NavIcons";

type Props = {
  active: AppView;
  hasSession: boolean;
  isPlaying: boolean;
  onNavigate: (view: AppView) => void;
};

const NAV_ITEMS: { id: AppView; labelKey: string; Icon: typeof IconHome }[] = [
  { id: "home", labelKey: "nav.home", Icon: IconHome },
  { id: "audiobooks", labelKey: "nav.audiobooks", Icon: IconBooks },
  { id: "music", labelKey: "nav.music", Icon: IconMusic },
  { id: "playlists", labelKey: "nav.playlists", Icon: IconList },
];

export function AppNav({ active, hasSession, isPlaying, onNavigate }: Props) {
  const { t } = useI18n();

  return (
    <nav className="app-nav" aria-label={t("nav.aria")}>
      <ul className="app-nav-list">
        {NAV_ITEMS.map((item) => (
          <li key={item.id}>
            <button
              type="button"
              className={`app-nav-btn${active === item.id ? " app-nav-btn--active" : ""}`}
              aria-current={active === item.id ? "page" : undefined}
              onClick={() => onNavigate(item.id)}
            >
              <item.Icon className="app-nav-icon" />
              <span className="app-nav-label">{t(item.labelKey)}</span>
            </button>
          </li>
        ))}
        {hasSession ? (
          <li>
            <button
              type="button"
              className={`app-nav-btn app-nav-btn--now${active === "nowPlaying" ? " app-nav-btn--active" : ""}`}
              aria-current={active === "nowPlaying" ? "page" : undefined}
              onClick={() => onNavigate("nowPlaying")}
            >
              <IconNowPlaying className="app-nav-icon" playing={isPlaying} />
              <span className="app-nav-label">{t("nav.nowPlaying")}</span>
            </button>
          </li>
        ) : null}
      </ul>
    </nav>
  );
}
