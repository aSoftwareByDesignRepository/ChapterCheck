import { describe, expect, it, vi } from "vitest";
import axe from "axe-core";
import { SleepTimerSheet } from "./SleepTimerSheet";
import { renderWithProviders } from "../test/renderWithProviders";
import "../styles.css";

async function expectWcag21Aa(container: HTMLElement) {
  const result = await axe.run(container, {
    runOnly: { type: "tag", values: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"] },
  });
  expect(result.violations, JSON.stringify(result.violations, null, 2)).toEqual([]);
}

describe("Sleep timer sheet", () => {
  it("one tap on a preset starts the timer; turn-off is disabled when idle", async () => {
    const onPick = vi.fn();
    const onOff = vi.fn();
    const { container, getByRole } = renderWithProviders(
      <SleepTimerSheet
        remainingLabel={null}
        stopAfterTrack={false}
        onPickMinutes={onPick}
        onTurnOff={onOff}
        onStopAfterChange={() => undefined}
      />,
    );
    getByRole("button", { name: "15 minutes" }).click();
    expect(onPick).toHaveBeenCalledWith(15);
    expect(getByRole("button", { name: "Turn timer off" })).toHaveProperty("disabled", true);
    await expectWcag21Aa(container);
  });

  it("turn off is available when a timer is running", async () => {
    const onOff = vi.fn();
    const { getByRole } = renderWithProviders(
      <SleepTimerSheet
        remainingLabel="12:00"
        stopAfterTrack={false}
        onPickMinutes={() => undefined}
        onTurnOff={onOff}
        onStopAfterChange={() => undefined}
      />,
    );
    const off = getByRole("button", { name: "Turn timer off" });
    expect(off).toHaveProperty("disabled", false);
    off.click();
    expect(onOff).toHaveBeenCalled();
  });

  it("a second tap while the first preset is still starting is ignored", async () => {
    let release: (() => void) | undefined;
    const onPick = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          release = resolve;
        }),
    );
    const { getByRole } = renderWithProviders(
      <SleepTimerSheet
        remainingLabel={null}
        stopAfterTrack={false}
        onPickMinutes={onPick}
        onTurnOff={() => undefined}
        onStopAfterChange={() => undefined}
      />,
    );
    const fifteen = getByRole("button", { name: "15 minutes" });
    fifteen.click();
    fifteen.click();
    getByRole("button", { name: "30 minutes" }).click();
    expect(onPick).toHaveBeenCalledTimes(1);
    expect(onPick).toHaveBeenCalledWith(15);
    release?.();
  });
});
