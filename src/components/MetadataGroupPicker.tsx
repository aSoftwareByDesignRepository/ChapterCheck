import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useState } from "react";
import { useI18n } from "../i18n/I18nContext";
import type { AddToPlaylistBulkResult, MetadataGroupDto, MetadataGroupKind } from "../types/catalog";

type AddMode = {
  mode: "add";
  playlistId: number;
  onDone: (result: AddToPlaylistBulkResult) => void;
};

type CreateMode = {
  mode: "create";
  onCreated: (playlistId: number) => void;
};

type Props = (AddMode | CreateMode) & {
  onClose: () => void;
};

const GROUP_KINDS: MetadataGroupKind[] = [
  "album",
  "artist",
  "audiobook",
  "author",
  "narrator",
  "series",
];

export function MetadataGroupPicker(props: Props) {
  const { t } = useI18n();
  const { onClose } = props;
  const [groupKind, setGroupKind] = useState<MetadataGroupKind>("album");
  const [search, setSearch] = useState("");
  const [groups, setGroups] = useState<MetadataGroupDto[]>([]);
  const [loading, setLoading] = useState(true);
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [status, setStatus] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const list = await invoke<MetadataGroupDto[]>("list_metadata_groups", {
        groupKind,
        search: search.trim() || null,
        limit: 1000,
        offset: 0,
      });
      setGroups(list);
    } catch (e) {
      setError(String(e));
      setGroups([]);
    } finally {
      setLoading(false);
    }
  }, [groupKind, search]);

  useEffect(() => {
    const tmr = window.setTimeout(() => void load(), search ? 200 : 0);
    return () => window.clearTimeout(tmr);
  }, [load, search]);

  useEffect(() => {
    setSearch("");
    setStatus(null);
    setError(null);
  }, [groupKind]);

  const handlePick = async (group: MetadataGroupDto) => {
    setBusyKey(group.group_key);
    setError(null);
    setStatus(null);
    try {
      if (props.mode === "add") {
        const result = await invoke<AddToPlaylistBulkResult>("add_metadata_group_to_playlist", {
          playlistId: props.playlistId,
          groupKind: group.group_kind,
          groupKey: group.group_key,
        });
        setStatus(
          result.tracks_skipped > 0
            ? t("playlists.metadataAddedPartial", {
                added: result.tracks_added,
                skipped: result.tracks_skipped,
              })
            : t("playlists.metadataAdded", { added: result.tracks_added }),
        );
        props.onDone(result);
      } else {
        const id = await invoke<number>("create_playlist_from_metadata_group", {
          groupKind: group.group_kind,
          groupKey: group.group_key,
        });
        props.onCreated(id);
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setBusyKey(null);
    }
  };

  const groupLabel = (kind: MetadataGroupKind) => t(`playlists.metadataGroup.${kind}`);

  return (
    <section className="metadata-picker" aria-labelledby="metadata-picker-title">
      <header className="metadata-picker-head">
        <div>
          <h2 id="metadata-picker-title" className="metadata-picker-title">
            {props.mode === "add" ? t("playlists.addByMetadata") : t("playlists.createFromMetadata")}
          </h2>
          <p className="metadata-picker-lead">
            {props.mode === "add" ? t("playlists.addByMetadataHint") : t("playlists.createFromMetadataHint")}
          </p>
        </div>
        <button type="button" className="btn btn-ghost btn-compact" onClick={onClose}>
          {t("modal.close")}
        </button>
      </header>

      <div className="metadata-picker-tabs" role="tablist" aria-label={t("playlists.metadataGroupLabel")}>
        {GROUP_KINDS.map((kind) => (
          <button
            key={kind}
            type="button"
            role="tab"
            aria-selected={groupKind === kind}
            className={`metadata-picker-tab${groupKind === kind ? " metadata-picker-tab--active" : ""}`}
            onClick={() => setGroupKind(kind)}
          >
            {groupLabel(kind)}
          </button>
        ))}
      </div>

      <input
        type="search"
        className="catalog-search metadata-picker-search"
        placeholder={t("playlists.metadataSearchPlaceholder", { group: groupLabel(groupKind) })}
        value={search}
        onChange={(e) => setSearch(e.target.value)}
        aria-label={t("playlists.metadataSearchPlaceholder", { group: groupLabel(groupKind) })}
      />

      {error ? (
        <p className="view-error" role="alert">
          {error}
        </p>
      ) : null}
      {status ? (
        <p className="playlist-import-status" role="status">
          {status}
        </p>
      ) : null}

      {loading ? (
        <p className="view-loading" aria-live="polite">
          {t("home.loading")}
        </p>
      ) : groups.length === 0 ? (
        <p className="view-empty-body">{t("playlists.metadataEmpty")}</p>
      ) : (
        <ul className="metadata-picker-list">
          {groups.map((group) => (
            <li key={group.group_key} className="metadata-picker-item">
              <div className="metadata-picker-item-text">
                <span className="metadata-picker-item-name">{group.label}</span>
                <span className="metadata-picker-item-meta">
                  {group.subtitle ? `${group.subtitle} · ` : ""}
                  {t("playlists.trackCount", { count: group.track_count })}
                </span>
              </div>
              <button
                type="button"
                className="btn btn-secondary btn-compact"
                disabled={busyKey != null}
                onClick={() => void handlePick(group)}
              >
                {busyKey === group.group_key
                  ? t("playlists.metadataAdding")
                  : props.mode === "add"
                    ? t("playlists.metadataAddBtn")
                    : t("playlists.metadataCreateBtn")}
              </button>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
