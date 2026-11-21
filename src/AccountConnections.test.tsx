import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { AccountConnections } from "./AccountConnections";
import type { Settings } from "./types";

function makeSettings(overrides: Partial<Settings> = {}): Settings {
  return {
    adult_content: false,
    game_version_warnings: true,
    autoclick_free_download: false,
    last_active_game_id: null,
    theme: "dark",
    theme_style: "app",
    has_api_key: false,
    has_modio_api_key: false,
    initial_setup_complete: true,
    nexus_website_signed_in: false,
    modio_website_signed_in: false,
    user: null,
    config_dir: "/cfg",
    data_dir: "/data",
    cache_dir: "/cache",
    ...overrides,
  };
}

function renderConnections(settings: Settings | null) {
  const html = renderToStaticMarkup(
    <AccountConnections
      settings={settings}
      apiKeyInput=""
      onApiKeyInput={() => {}}
      onSaveApiKey={() => {}}
      onClearApiKey={() => {}}
      modioKeyInput=""
      onModioKeyInput={() => {}}
      onSaveModioApiKey={() => {}}
      onClearModioApiKey={() => {}}
      signInDisabled={false}
      onSignIn={() => {}}
      onModioSignIn={() => {}}
      onClearWebsiteSession={() => {}}
      onClearModioWebsiteSession={() => {}}
    />,
  );
  const root = document.createElement("div");
  root.innerHTML = html;
  return root;
}

describe("AccountConnections", () => {
  it("shows both Nexus website and API key as not set when signed out", () => {
    const root = renderConnections(makeSettings());
    const nexus = root.querySelector('[data-testid="nexus-status-card"]');
    const pills = [...(nexus?.querySelectorAll(".pill") ?? [])].map((n) => ({
      text: n.textContent,
      ok: n.classList.contains("ok"),
      warn: n.classList.contains("warn"),
    }));

    expect(pills).toEqual([
      { text: "Website: Not signed in", ok: false, warn: true },
      { text: "API key: Not set", ok: false, warn: true },
    ]);
  });

  it("shows website signed in and API key not set when key retrieval is pending", () => {
    const root = renderConnections(
      makeSettings({ nexus_website_signed_in: true }),
    );
    const nexus = root.querySelector('[data-testid="nexus-status-card"]');
    const pills = [...(nexus?.querySelectorAll(".pill") ?? [])].map((n) => ({
      text: n.textContent,
      ok: n.classList.contains("ok"),
      warn: n.classList.contains("warn"),
    }));

    expect(pills).toEqual([
      { text: "Website: Signed in", ok: true, warn: false },
      { text: "API key: Not set", ok: false, warn: true },
    ]);
  });

  it("shows website not signed in when only a manual Nexus API key is saved", () => {
    const root = renderConnections(
      makeSettings({
        has_api_key: true,
        nexus_website_signed_in: false,
        user: {
          user_id: 1,
          name: "lewbydoo",
          is_premium: false,
          is_supporter: false,
          key: "test-key",
        },
      }),
    );
    const nexus = root.querySelector('[data-testid="nexus-status-card"]');
    expect(nexus?.textContent).toContain("Signed in as lewbydoo");
    const pills = [...(nexus?.querySelectorAll(".pill") ?? [])].map((n) => ({
      text: n.textContent,
      ok: n.classList.contains("ok"),
      warn: n.classList.contains("warn"),
    }));

    expect(pills).toEqual([
      { text: "Free — Download Assist", ok: false, warn: true },
      { text: "Website: Not signed in", ok: false, warn: true },
      { text: "API key: Saved", ok: true, warn: false },
    ]);
  });

  it("shows unverified API key status when has_api_key is true without a validated user", () => {
    const root = renderConnections(
      makeSettings({
        has_api_key: true,
        nexus_website_signed_in: true,
        user: null,
      }),
    );
    const nexus = root.querySelector('[data-testid="nexus-status-card"]');
    const pills = [...(nexus?.querySelectorAll(".pill") ?? [])].map((n) => ({
      text: n.textContent,
      ok: n.classList.contains("ok"),
      warn: n.classList.contains("warn"),
    }));

    expect(pills).toEqual([
      { text: "Website: Signed in", ok: true, warn: false },
      { text: "API key: Saved (unverified)", ok: false, warn: true },
    ]);
  });

  it("shows both Nexus website and API key as ok for a fully connected Premium user", () => {
    const root = renderConnections(
      makeSettings({
        has_api_key: true,
        nexus_website_signed_in: true,
        user: {
          user_id: 42,
          name: "lewbydoo",
          is_premium: true,
          is_supporter: true,
          key: "test-key",
        },
      }),
    );
    const nexus = root.querySelector('[data-testid="nexus-status-card"]');
    const pills = [...(nexus?.querySelectorAll(".pill") ?? [])].map((n) => ({
      text: n.textContent,
      ok: n.classList.contains("ok"),
    }));

    expect(pills).toEqual([
      { text: "Premium", ok: true },
      { text: "Website: Signed in", ok: true },
      { text: "API key: Saved", ok: true },
    ]);
  });

  it("shows mod.io API key and website session status in both unset and saved states", () => {
    const unsetRoot = renderConnections(
      makeSettings({ has_modio_api_key: false }),
    );
    const unsetModio = unsetRoot.querySelector('[data-testid="modio-status-card"]');
    expect(
      [...(unsetModio?.querySelectorAll(".pill") ?? [])].map((n) => ({
        text: n.textContent,
        muted: n.classList.contains("muted"),
      })),
    ).toEqual([
      { text: "API key: Not set (optional)", muted: true },
      { text: "Website session: Not saved", muted: true },
    ]);
    expect(unsetRoot.textContent).not.toContain("Clear mod.io website session");

    const savedRoot = renderConnections(
      makeSettings({ has_modio_api_key: true, modio_website_signed_in: true }),
    );
    const savedModio = savedRoot.querySelector('[data-testid="modio-status-card"]');
    expect(
      [...(savedModio?.querySelectorAll(".pill") ?? [])].map((n) => ({
        text: n.textContent,
        ok: n.classList.contains("ok"),
        muted: n.classList.contains("muted"),
      })),
    ).toEqual([
      { text: "API key: Saved", ok: true, muted: false },
      { text: "Website session: Saved", ok: true, muted: false },
    ]);
    expect(savedRoot.textContent).toContain("Clear mod.io website session");
  });
});
