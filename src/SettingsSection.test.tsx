import { createRoot, type Root } from "react-dom/client";
import { act, type ReactElement } from "react";
import { describe, expect, it, vi } from "vitest";

import { SettingsSection, SettingsToggle } from "./SettingsSection";

function sections(host: HTMLElement): HTMLDetailsElement[] {
  return [
    ...host.querySelectorAll<HTMLDetailsElement>("details.settings-section"),
  ];
}

async function flushToggles() {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

async function waitUntilClosed(section: HTMLDetailsElement) {
  const started = Date.now();
  while (section.open) {
    if (Date.now() - started > 1000) {
      throw new Error("section stayed open after the close animation");
    }
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 50));
    });
  }
}

describe("SettingsSection", () => {
  async function render(ui: ReactElement) {
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root: Root = createRoot(host);
    await act(async () => {
      root.render(ui);
    });
    await flushToggles();
    return {
      host,
      async unmount() {
        await act(async () => root.unmount());
        host.remove();
      },
    };
  }

  function menu() {
    return (
      <>
        <SettingsSection title="App updates" defaultOpen>
          <p>updates</p>
        </SettingsSection>
        <SettingsSection title="Appearance">
          <p>look</p>
        </SettingsSection>
        <SettingsSection title="Accounts">
          <p>accounts</p>
        </SettingsSection>
      </>
    );
  }

  it("starts with the default section open and the others closed", async () => {
    const view = await render(menu());
    const [updates, appearance, accounts] = sections(view.host);
    expect(updates.open).toBe(true);
    expect(appearance.open).toBe(false);
    expect(accounts.open).toBe(false);
    await view.unmount();
  });

  it("closes the open section when another one opens", async () => {
    const view = await render(menu());
    const [updates, appearance, accounts] = sections(view.host);
    await act(async () => {
      appearance.querySelector("summary")?.click();
    });
    expect(appearance.open).toBe(true);
    await waitUntilClosed(updates);
    expect(updates.open).toBe(false);
    expect(accounts.open).toBe(false);

    await act(async () => {
      accounts.querySelector("summary")?.click();
    });
    expect(accounts.open).toBe(true);
    await waitUntilClosed(appearance);
    expect(updates.open).toBe(false);
    expect(appearance.open).toBe(false);
    expect(accounts.open).toBe(true);
    await view.unmount();
  });

  it("lets the open section collapse", async () => {
    const view = await render(menu());
    const [updates, appearance] = sections(view.host);
    await act(async () => {
      updates.querySelector("summary")?.click();
    });
    await waitUntilClosed(updates);
    expect(updates.open).toBe(false);
    expect(appearance.open).toBe(false);
    await view.unmount();
  });
});

describe("SettingsToggle", () => {
  it("renders a switch button with aria-checked and toggles on click", async () => {
    const onChange = vi.fn();
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root: Root = createRoot(host);

    await act(async () => {
      root.render(
        <SettingsToggle checked={false} onChange={onChange}>
          Show adult content
        </SettingsToggle>,
      );
    });

    const toggle = host.querySelector<HTMLButtonElement>(
      'button[role="switch"]',
    );
    expect(toggle).toBeTruthy();
    expect(toggle?.getAttribute("aria-checked")).toBe("false");
    expect(toggle?.textContent).toContain("Show adult content");

    await act(async () => {
      toggle!.click();
    });
    expect(onChange).toHaveBeenCalledWith(true);

    await act(async () => {
      root.render(
        <SettingsToggle checked={true} disabled onChange={onChange}>
          Show adult content
        </SettingsToggle>,
      );
    });
    expect(toggle?.getAttribute("aria-checked")).toBe("true");
    expect(toggle?.disabled).toBe(true);

    await act(async () => {
      toggle!.click();
    });
    expect(onChange).toHaveBeenCalledTimes(1);

    await act(async () => root.unmount());
    host.remove();
  });
});
