import { describe, expect, it } from "vitest";
import { LibrarySidebar } from "./LibrarySidebar";
import { renderWithProviders } from "../test/renderWithProviders";

describe("Library sidebar", () => {
  it("renders nothing when there is no library (welcome lives on Home)", () => {
    const { container } = renderWithProviders(
      <LibrarySidebar linkedFolderCount={0} onLinkFolder={() => undefined} onManageLibrary={() => undefined} />,
    );
    expect(container.textContent?.trim()).toBe("");
  });

  it("shows add-folder and manage when a library exists", () => {
    const { getByRole } = renderWithProviders(
      <LibrarySidebar linkedFolderCount={2} onLinkFolder={() => undefined} onManageLibrary={() => undefined} />,
    );
    expect(getByRole("button", { name: "Add folder" })).toBeTruthy();
    expect(getByRole("button", { name: "Manage folders" })).toBeTruthy();
  });
});
