import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type MouseEvent,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";

export type ContextMenuEntry =
  | { type: "separator" }
  | {
      id: string;
      label: string;
      onClick: () => void;
      disabled?: boolean;
      danger?: boolean;
    };

type MenuState = {
  x: number;
  y: number;
  items: ContextMenuEntry[];
};

type ContextMenuApi = {
  openContextMenu: (event: MouseEvent, items: ContextMenuEntry[]) => void;
  openContextMenuAt: (x: number, y: number, items: ContextMenuEntry[]) => void;
  closeContextMenu: () => void;
};

const ContextMenuCtx = createContext<ContextMenuApi | null>(null);

export function useContextMenu() {
  const ctx = useContext(ContextMenuCtx);
  if (!ctx) {
    throw new Error("useContextMenu requires ContextMenuProvider");
  }
  return ctx;
}

function actionableItems(items: ContextMenuEntry[]) {
  return items.filter((it): it is Extract<ContextMenuEntry, { id: string }> => it.type !== "separator");
}

function ContextMenuPanel({ state, onClose }: { state: MenuState; onClose: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ left: state.x, top: state.y });
  const [focusIndex, setFocusIndex] = useState(0);

  const entries = actionableItems(state.items);

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    let left = state.x;
    let top = state.y;
    if (left + rect.width > window.innerWidth - 8) {
      left = Math.max(8, window.innerWidth - rect.width - 8);
    }
    if (top + rect.height > window.innerHeight - 8) {
      top = Math.max(8, window.innerHeight - rect.height - 8);
    }
    setPos({ left, top });
  }, [state.x, state.y, state.items]);

  useEffect(() => {
    setFocusIndex(0);
    const first = ref.current?.querySelector<HTMLButtonElement>(
      'button[role="menuitem"]:not([disabled])',
    );
    first?.focus();
  }, [state.items]);

  useEffect(() => {
    const onPointer = (e: PointerEvent) => {
      if (ref.current?.contains(e.target as Node)) return;
      onClose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    const onScroll = () => onClose();
    window.addEventListener("pointerdown", onPointer, true);
    window.addEventListener("keydown", onKey);
    window.addEventListener("scroll", onScroll, true);
    return () => {
      window.removeEventListener("pointerdown", onPointer, true);
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("scroll", onScroll, true);
    };
  }, [onClose]);

  const onMenuKeyDown = (e: ReactKeyboardEvent<HTMLDivElement>) => {
    if (entries.length === 0) return;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setFocusIndex((i) => {
        let next = (i + 1) % entries.length;
        while (entries[next]?.disabled && next !== i) {
          next = (next + 1) % entries.length;
        }
        return next;
      });
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setFocusIndex((i) => {
        let next = (i - 1 + entries.length) % entries.length;
        while (entries[next]?.disabled && next !== i) {
          next = (next - 1 + entries.length) % entries.length;
        }
        return next;
      });
    } else if (e.key === "Home") {
      e.preventDefault();
      setFocusIndex(0);
    } else if (e.key === "End") {
      e.preventDefault();
      setFocusIndex(entries.length - 1);
    }
  };

  useEffect(() => {
    const buttons = ref.current?.querySelectorAll<HTMLButtonElement>(
      'button[role="menuitem"]:not([disabled])',
    );
    buttons?.[focusIndex]?.focus();
  }, [focusIndex, state.items]);

  let actionableIdx = -1;

  return createPortal(
    <div
      ref={ref}
      className="context-menu"
      role="menu"
      style={{ left: pos.left, top: pos.top }}
      onKeyDown={onMenuKeyDown}
    >
      {state.items.map((item, i) => {
        if (item.type === "separator") {
          return <div key={`sep-${i}`} className="context-menu-sep" role="separator" />;
        }
        actionableIdx += 1;
        return (
          <button
            key={item.id}
            type="button"
            role="menuitem"
            tabIndex={actionableIdx === focusIndex ? 0 : -1}
            className={`context-menu-item${item.danger ? " context-menu-item--danger" : ""}`}
            disabled={item.disabled}
            onClick={() => {
              if (item.disabled) return;
              onClose();
              item.onClick();
            }}
          >
            {item.label}
          </button>
        );
      })}
    </div>,
    document.body,
  );
}

export function ContextMenuProvider({ children }: { children: ReactNode }) {
  const [state, setState] = useState<MenuState | null>(null);

  const closeContextMenu = useCallback(() => setState(null), []);

  const openContextMenuAt = useCallback((x: number, y: number, items: ContextMenuEntry[]) => {
    const actionable = actionableItems(items);
    if (actionable.length === 0) return;
    setState({ x, y, items });
  }, []);

  const openContextMenu = useCallback(
    (event: MouseEvent, items: ContextMenuEntry[]) => {
      event.preventDefault();
      event.stopPropagation();
      openContextMenuAt(event.clientX, event.clientY, items);
    },
    [openContextMenuAt],
  );

  return (
    <ContextMenuCtx.Provider value={{ openContextMenu, openContextMenuAt, closeContextMenu }}>
      {children}
      {state ? <ContextMenuPanel state={state} onClose={closeContextMenu} /> : null}
    </ContextMenuCtx.Provider>
  );
}
