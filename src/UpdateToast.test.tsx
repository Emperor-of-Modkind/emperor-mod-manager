import { createRoot, type Root } from "react-dom/client";
import { act, type ReactElement } from "react";
import { describe, expect, it } from "vitest";

import { CheckingFriendUpdatesToast, CheckingUpdatesToast, UpdateToast } from "./UpdateToast";

async function render(ui: ReactElement) {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root: Root = createRoot(host);
  await act(async () => {
    root.render(ui);
  });
  return {
    host,
    async unmount() {
      await act(async () => root.unmount());
      host.remove();
    },
  };
}

function buttonNamed(host: HTMLElement, label: string) {
  return [...host.querySelectorAll("button")].find((button) => button.textContent === label);
}

describe("UpdateToast", () => {
  it("offers Update and reports Update and Later", async () => {
    let updated = false;
    let later = false;
    const view = await render(
      <UpdateToast
        show
        version="1.2.3"
        progress={null}
        canInstall
        checking={false}
        installing={false}
        onUpdate={() => {
          updated = true;
        }}
        onOpenRelease={() => {}}
        onLater={() => {
          later = true;
        }}
      />,
    );
    expect(view.host.textContent).toContain("Update available");
    expect(view.host.textContent).toContain("v1.2.3");
    expect(view.host.querySelector(".throbber")).toBeNull();
    expect(buttonNamed(view.host, "Open release")).toBeUndefined();

    await act(async () => {
      buttonNamed(view.host, "Update")?.click();
    });
    await act(async () => {
      buttonNamed(view.host, "Later")?.click();
    });
    expect(updated).toBe(true);
    expect(later).toBe(true);
    await view.unmount();
  });

  it("offers Open release when this install cannot be replaced", async () => {
    let opened = false;
    const view = await render(
      <UpdateToast
        show
        version="1.2.3"
        progress={null}
        canInstall={false}
        checking={false}
        installing={false}
        onUpdate={() => {}}
        onOpenRelease={() => {
          opened = true;
        }}
        onLater={() => {}}
      />,
    );
    expect(buttonNamed(view.host, "Update")).toBeUndefined();
    expect(view.host.querySelector(".throbber")).toBeNull();
    await act(async () => {
      buttonNamed(view.host, "Open release")?.click();
    });
    expect(opened).toBe(true);
    await view.unmount();
  });

  it("shows progress and keeps Later disabled while installing", async () => {
    const view = await render(
      <UpdateToast
        show
        version="1.2.3"
        progress="Downloading…"
        canInstall
        checking={false}
        installing
        onUpdate={() => {}}
        onOpenRelease={() => {}}
        onLater={() => {}}
      />,
    );
    expect(view.host.textContent).toContain("Downloading…");
    expect(view.host.querySelector(".throbber")).not.toBeNull();
    expect(buttonNamed(view.host, "Updating…")?.disabled).toBe(true);
    expect(buttonNamed(view.host, "Later")?.disabled).toBe(true);
    await view.unmount();
  });
});

describe("CheckingUpdatesToast", () => {
  it("says it is checking and includes the staged count", async () => {
    const view = await render(<CheckingUpdatesToast show stagedCount={84} />);
    expect(view.host.querySelector("[role='status']")?.textContent).toContain(
      "Checking for mod updates…",
    );
    expect(view.host.querySelector(".throbber")).not.toBeNull();
    expect(view.host.textContent).toContain("84 staged mods");
    expect(view.host.querySelector("button")).toBeNull();
    await view.unmount();
  });

  it("uses the singular when one mod is staged", async () => {
    const view = await render(<CheckingUpdatesToast show stagedCount={1} />);
    expect(view.host.textContent).toContain("1 staged mod");
    expect(view.host.textContent).not.toContain("mods");
    await view.unmount();
  });

  it("omits the count until the staged list is loaded", async () => {
    const view = await render(<CheckingUpdatesToast show stagedCount={null} />);
    expect(view.host.textContent).toContain("Checking for mod updates…");
    expect(view.host.textContent).not.toContain("staged");
    await view.unmount();
  });
});

describe("CheckingFriendUpdatesToast", () => {
  it("shows checking for friend updates status", async () => {
    const view = await render(<CheckingFriendUpdatesToast show />);
    expect(view.host.querySelector("[role='status']")?.textContent).toContain(
      "Checking for friend updates…",
    );
    expect(view.host.querySelector(".throbber")).not.toBeNull();
    expect(view.host.querySelector("button")).toBeNull();
    await view.unmount();
  });
});
