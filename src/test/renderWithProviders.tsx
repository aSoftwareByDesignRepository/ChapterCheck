import type { ReactElement, ReactNode } from "react";
import { render, type RenderOptions } from "@testing-library/react";
import { I18nProvider } from "../i18n/I18nContext";
import { AddToPlaylistProvider } from "../context/AddToPlaylistContext";
import { ContextMenuProvider } from "../context/ContextMenuContext";
import { LOCALE_STORAGE_KEY } from "../i18n/types";

export function renderWithProviders(ui: ReactElement, options?: Omit<RenderOptions, "wrapper">) {
  try {
    localStorage.setItem(LOCALE_STORAGE_KEY, "en");
  } catch {
    /* jsdom */
  }
  function Wrapper({ children }: { children: ReactNode }) {
    return (
      <I18nProvider>
        <AddToPlaylistProvider>
          <ContextMenuProvider>{children}</ContextMenuProvider>
        </AddToPlaylistProvider>
      </I18nProvider>
    );
  }
  return render(ui, { wrapper: Wrapper, ...options });
}
