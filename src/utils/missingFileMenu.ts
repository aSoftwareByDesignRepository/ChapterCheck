import type { ContextMenuEntry } from "../context/ContextMenuContext";

type Handlers = {
  onRelink: (fileId: number) => void | Promise<void>;
  onRemove: (fileId: number, title: string) => void;
};

export function missingFileContextEntries(
  fileId: number,
  title: string,
  handlers: Handlers,
  t: (key: string, params?: Record<string, string | number>) => string,
): ContextMenuEntry[] {
  return [
    {
      id: "relink",
      label: t("catalog.relinkFile"),
      onClick: () => void handlers.onRelink(fileId),
    },
    { type: "separator" },
    {
      id: "remove",
      label: t("catalog.removeFromLibrary"),
      danger: true,
      onClick: () => handlers.onRemove(fileId, title),
    },
  ];
}
