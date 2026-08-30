import { afterEach, beforeEach } from "vitest";
import { cleanup } from "@testing-library/react";

beforeEach(() => {
  document.documentElement.dataset.theme = "dark";
  document.documentElement.style.colorScheme = "dark";
});

afterEach(() => {
  cleanup();
});
