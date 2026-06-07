import { invoke } from "@tauri-apps/api/core";
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";
import type { ContextMenuEntry } from "./ContextMenuContext";
import { useI18n } from "../i18n/I18nContext";
import type { CollectionDetailDto, PlaylistSummaryDto } from "../types/catalog";

export type AddToPlaylistTarget =
  | { fileIds: number[] }
  | { fileId: number }
  | { path: string }
  | { collectionId: number; title?: string };

type DialogState = {
  target: AddToPlaylistTarget;
  fileIds: number[] | null;
  subtitle: string;
  blocked: boolean;
  error: string | null;
  feedback: string | null;
  busyId: number | null;
};

type AddToPlaylistApi = {
  openAddToPlaylist: (target: AddToPlaylistTarget) => void;
  playlistContextEntries: (target: AddToPlaylistTarget) => ContextMenuEntry[];
  appendPlaylistContextEntries: (
    base: ContextMenuEntry[],
    target: AddToPlaylistTarget,
  ) => ContextMenuEntry[];
  invalidatePlaylists: () => void;
};

const AddToPlaylistCtx = createContext<AddToPlaylistApi | null>(null);

async function resolveFileIds(target: AddToPlaylistTarget): Promise<number[]> {
  if ("fileIds" in target) {
    return target.fileIds;
  }
  if ("fileId" in target) {
    return [target.fileId];
  }
  if ("path" in target) {
    const id = await invoke<number | null>("find_collection_file_id", { path: target.path });
    return id != null ? [id] : [];
  }
  const detail = await invoke<CollectionDetailDto>("get_collection_detail", {
    collectionId: target.collectionId,
  });
  return detail.files.filter((f) => !f.unavailable).map((f) => f.id);
}

function subtitleForTarget(target: AddToPlaylistTarget, count: number, t: (k: string, p?: Record<string, string | number>) => string): string {
  if ("collectionId" in target && target.title) {
    return t("addToPlaylist.subtitleCollection", { title: target.title, count });
  }
  if (count === 1) {
    return t("addToPlaylist.subtitleOne");
  }
  return t("addToPlaylist.subtitleMany", { count });
}

function AddToPlaylistDialog({
  state,
  playlists,
  onClose,
  onPick,
  onCreate,
}: {
  state: DialogState;
  playlists: PlaylistSummaryDto[];
  onClose: () => void;
  onPick: (playlistId: number) => void;
  onCreate: (name: string) => void;
}) {
  const { t } = useI18n();
  const [newName, setNewName] = useState("");
  const [creating, setCreating] = useState(false);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const showCreate = playlists.length === 0 || creating;

  return createPortal(
    <div className="add-pl-backdrop" role="presentation" onClick={onClose}>
      <div
        className="add-pl-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="add-pl-dialog-title"
        onClick={(e) => e.stopPropagation()}
      >
        <header className="add-pl-dialog-head">
          <div>
            <h3 id="add-pl-dialog-title" className="add-pl-dialog-title">
              {t("catalog.addToPlaylist")}
            </h3>
            <p className="add-pl-dialog-sub">{state.subtitle}</p>
          </div>
          <button type="button" className="add-pl-dialog-close" onClick={onClose} aria-label={t("modal.close")}>
            ×
          </button>
        </header>

        {state.error ? (
          <p className="add-pl-dialog-msg add-pl-dialog-msg--error" role="alert">
            {state.error}
          </p>
        ) : null}

        {state.blocked ? (
          <p className="add-pl-dialog-msg" role="status">
            {t("catalog.addToPlaylistNeedLibrary")}
          </p>
        ) : state.fileIds == null ? (
          <p className="add-pl-dialog-msg" aria-live="polite">
            {t("addToPlaylist.loading")}
          </p>
        ) : state.feedback ? (
          <p className="add-pl-dialog-msg add-pl-dialog-msg--ok" role="status">
            {state.feedback}
          </p>
        ) : (
          <>
            {showCreate ? (
              <form
                className="add-pl-dialog-create"
                onSubmit={(e) => {
                  e.preventDefault();
                  const name = newName.trim();
                  if (!name) return;
                  onCreate(name);
                  setNewName("");
                }}
              >
                <label className="add-pl-dialog-create-label">
                  <span className="field-label">{t("addToPlaylist.newPlaylist")}</span>
                  <input
                    type="text"
                    className="catalog-search"
                    value={newName}
                    onChange={(e) => setNewName(e.target.value)}
                    placeholder={t("playlists.namePlaceholder")}
                    autoFocus
                  />
                </label>
                <button type="submit" className="btn btn-primary" disabled={!newName.trim()}>
                  {t("playlists.create")}
                </button>
              </form>
            ) : null}

            {playlists.length > 0 ? (
              <ul className="add-pl-dialog-list" role="listbox" aria-label={t("catalog.pickPlaylist")}>
                {playlists.map((pl) => (
                  <li key={pl.id}>
                    <button
                      type="button"
                      role="option"
                      className="add-pl-dialog-item"
                      disabled={state.busyId != null}
                      onClick={() => onPick(pl.id)}
                    >
                      <span className="add-pl-dialog-item-name">{pl.name}</span>
                      <span className="add-pl-dialog-item-meta">
                        {t("playlists.trackCount", { count: pl.track_count })}
                      </span>
                    </button>
                  </li>
                ))}
              </ul>
            ) : null}

            {playlists.length > 0 && !creating ? (
              <button
                type="button"
                className="btn btn-ghost add-pl-dialog-new"
                onClick={() => setCreating(true)}
              >
                {t("addToPlaylist.newPlaylist")}
              </button>
            ) : null}
          </>
        )}
      </div>
    </div>,
    document.body,
  );
}

export function AddToPlaylistProvider({ children }: { children: ReactNode }) {
  const { t } = useI18n();
  const cache = useRef<PlaylistSummaryDto[] | null>(null);
  const [playlists, setPlaylists] = useState<PlaylistSummaryDto[]>([]);
  const [dialog, setDialog] = useState<DialogState | null>(null);

  const refreshPlaylists = useCallback(async () => {
    const list = await invoke<PlaylistSummaryDto[]>("list_playlists").catch(() => []);
    cache.current = list;
    setPlaylists(list);
    return list;
  }, []);

  const invalidatePlaylists = useCallback(() => {
    cache.current = null;
    void refreshPlaylists();
  }, [refreshPlaylists]);

  useEffect(() => {
    void refreshPlaylists();
  }, [refreshPlaylists]);

  const openAddToPlaylist = useCallback(
    (target: AddToPlaylistTarget) => {
      setDialog({
        target,
        fileIds: null,
        subtitle: t("addToPlaylist.loading"),
        blocked: false,
        error: null,
        feedback: null,
        busyId: null,
      });
      void resolveFileIds(target)
        .then((fileIds) => {
          if (fileIds.length === 0) {
            setDialog((d) =>
              d
                ? {
                    ...d,
                    fileIds: [],
                    blocked: true,
                    subtitle: t("catalog.addToPlaylistNeedLibrary"),
                  }
                : d,
            );
            return;
          }
          setDialog((d) =>
            d
              ? {
                  ...d,
                  fileIds,
                  blocked: false,
                  subtitle: subtitleForTarget(target, fileIds.length, t),
                }
              : d,
          );
        })
        .catch((e) => {
          setDialog((d) => (d ? { ...d, error: String(e), fileIds: [] } : d));
        });
    },
    [t],
  );

  const addToPlaylist = useCallback(
    async (playlistId: number, fileIds: number[]) => {
      setDialog((d) => (d ? { ...d, busyId: playlistId, error: null } : d));
      try {
        for (const fid of fileIds) {
          await invoke("add_to_playlist", { playlistId, collectionFileId: fid });
        }
        const name = playlists.find((p) => p.id === playlistId)?.name ?? "";
        setDialog((d) =>
          d
            ? {
                ...d,
                busyId: null,
                feedback: t("addToPlaylist.added", { name }),
              }
            : d,
        );
        window.setTimeout(() => setDialog(null), 900);
      } catch (e) {
        setDialog((d) => (d ? { ...d, busyId: null, error: String(e) } : d));
      }
    },
    [playlists, t],
  );

  const createAndAdd = useCallback(
    async (name: string) => {
      if (!dialog?.fileIds?.length) return;
      setDialog((d) => (d ? { ...d, busyId: -1, error: null } : d));
      try {
        const id = await invoke<number>("create_playlist", { name, pin: false });
        invalidatePlaylists();
        await addToPlaylist(id, dialog.fileIds);
      } catch (e) {
        setDialog((d) => (d ? { ...d, busyId: null, error: String(e) } : d));
      }
    },
    [addToPlaylist, dialog?.fileIds, invalidatePlaylists],
  );

  const playlistContextEntries = useCallback(
    (target: AddToPlaylistTarget): ContextMenuEntry[] => [
      {
        id: "add-to-playlist",
        label: t("catalog.addToPlaylist"),
        onClick: () => openAddToPlaylist(target),
      },
    ],
    [openAddToPlaylist, t],
  );

  const appendPlaylistContextEntries = useCallback(
    (base: ContextMenuEntry[], target: AddToPlaylistTarget): ContextMenuEntry[] => [
      ...base,
      { type: "separator" },
      ...playlistContextEntries(target),
    ],
    [playlistContextEntries],
  );

  return (
    <AddToPlaylistCtx.Provider
      value={{
        openAddToPlaylist,
        playlistContextEntries,
        appendPlaylistContextEntries,
        invalidatePlaylists,
      }}
    >
      {children}
      {dialog ? (
        <AddToPlaylistDialog
          state={dialog}
          playlists={playlists}
          onClose={() => setDialog(null)}
          onPick={(id) => {
            if (dialog.fileIds?.length) void addToPlaylist(id, dialog.fileIds);
          }}
          onCreate={(name) => void createAndAdd(name)}
        />
      ) : null}
    </AddToPlaylistCtx.Provider>
  );
}

export function useAddToPlaylist() {
  const ctx = useContext(AddToPlaylistCtx);
  if (!ctx) {
    throw new Error("useAddToPlaylist requires AddToPlaylistProvider");
  }
  return ctx;
}
