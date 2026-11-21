import { createRoot, type Root } from "react-dom/client";
import { act, type ReactElement } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { WineOverrideNotice } from "./WineOverrideNotice";
import type { WineOverrideAdvice } from "./types";

const sampleAdvice: WineOverrideAdvice = {
  needs: [{ dll: "winmm", reason: "Required by mod loader" }],
  launch_options: 'WINEDLLOVERRIDES="winmm=n,b" %command%',
  typo: false,
  can_apply: true,
  summary: "Proton skips these native DLLs unless launch options tell Wine to load them.",
};

let mountedRoot: Root | null = null;
let mountedContainer: HTMLDivElement | null = null;

function renderNotice(element: ReactElement) {
  mountedContainer = document.createElement("div");
  document.body.appendChild(mountedContainer);
  mountedRoot = createRoot(mountedContainer);
  act(() => {
    mountedRoot!.render(element);
  });
  return mountedContainer;
}

afterEach(() => {
  if (mountedRoot) {
    act(() => {
      mountedRoot!.unmount();
    });
    mountedRoot = null;
  }
  if (mountedContainer) {
    mountedContainer.remove();
    mountedContainer = null;
  }
});

describe("WineOverrideNotice", () => {
  it("renders a Dismiss button when onDismiss is provided and invokes it", () => {
    const onDismiss = vi.fn();
    const container = renderNotice(
      <WineOverrideNotice
        advice={sampleAdvice}
        busy={false}
        onApply={vi.fn()}
        onDismiss={onDismiss}
      />,
    );

    const dismissBtn = container.querySelector('button[aria-label="Dismiss"]');
    expect(dismissBtn).not.toBeNull();
    act(() => {
      dismissBtn!.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });

  it("omits the Dismiss button when onDismiss is not provided (e.g. in Info modal)", () => {
    const container = renderNotice(
      <WineOverrideNotice advice={sampleAdvice} busy={false} onApply={vi.fn()} />,
    );

    expect(container.querySelector('button[aria-label="Dismiss"]')).toBeNull();
    expect(container.querySelector('button[aria-label="Copy"]')).not.toBeNull();
    expect(container.textContent).toContain("Set in Steam");
  });
});
