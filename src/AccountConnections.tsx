import type { Settings } from "./types";

type AccountConnectionsProps = {
  settings: Settings | null;
  apiKeyInput: string;
  onApiKeyInput: (value: string) => void;
  onSaveApiKey: () => void;
  onClearApiKey: () => void;
  modioKeyInput: string;
  onModioKeyInput: (value: string) => void;
  onSaveModioApiKey: () => void;
  onClearModioApiKey: () => void;
  signInDisabled: boolean;
  onSignIn: () => void;
  onModioSignIn: () => void;
  onClearWebsiteSession: () => void;
};

export function AccountConnections({
  settings,
  apiKeyInput,
  onApiKeyInput,
  onSaveApiKey,
  onClearApiKey,
  modioKeyInput,
  onModioKeyInput,
  onSaveModioApiKey,
  onClearModioApiKey,
  signInDisabled,
  onSignIn,
  onModioSignIn,
  onClearWebsiteSession,
}: AccountConnectionsProps) {
  return (
    <>
      <h2>Nexus Mods account</h2>
      <p>
        Sign in with your Nexus Mods account (use <strong>Stay signed in</strong>
        ). Your website session enables Free Download Assist and automatically
        retrieves your personal API key in the background.
      </p>
      {(settings?.user || settings?.nexus_website_signed_in) && (
        <div className="status-card">
          {settings?.user ? (
            <>
              Signed in as <strong>{settings.user.name}</strong>
              {settings.user.is_premium ? (
                <span className="pill ok">Premium</span>
              ) : (
                <span className="pill warn">Free — Download Assist</span>
              )}
            </>
          ) : (
            <span className="pill ok">Website session saved</span>
          )}
        </div>
      )}
      <div className="actions">
        <button
          type="button"
          disabled={signInDisabled}
          title={
            signInDisabled
              ? "Finish or cancel the Download Assist queue before signing in."
              : undefined
          }
          onClick={onSignIn}
        >
          Log in to Nexus
        </button>
        {settings?.nexus_website_signed_in && (
          <button type="button" onClick={onClearWebsiteSession}>
            Clear Nexus website session
          </button>
        )}
      </div>

      <details style={{ marginTop: "1rem" }}>
        <summary style={{ cursor: "pointer" }}>
          Advanced: Paste personal Nexus API key manually
        </summary>
        <p style={{ marginTop: "0.75rem" }}>
          Optional fallback if automatic key retrieval fails. Paste your key from{" "}
          <a
            href="https://www.nexusmods.com/users/myaccount?tab=api"
            target="_blank"
            rel="noreferrer"
          >
            Account → API Access
          </a>
          .
        </p>
        <div className="row">
          <input
            type="password"
            placeholder="Nexus API key"
            value={apiKeyInput}
            onChange={(e) => onApiKeyInput(e.target.value)}
          />
          <button onClick={onSaveApiKey} disabled={!apiKeyInput.trim()}>
            Save & validate
          </button>
        </div>
        {settings?.has_api_key && (
          <button className="danger" onClick={onClearApiKey}>
            Clear Nexus API key
          </button>
        )}
      </details>

      <h2 style={{ marginTop: "1.5rem" }}>mod.io</h2>
      <p>
        Optional read-only access for games with a mod.io game ID. Sign in with
        mod.io to retrieve your API key automatically in the background. OAuth,
        uploads, and ratings are not supported.
      </p>
      {settings?.has_modio_api_key && (
        <div className="status-card">
          <span className="pill ok">mod.io API key saved</span>
        </div>
      )}
      <div className="actions">
        <button
          type="button"
          disabled={signInDisabled}
          title={
            signInDisabled
              ? "Finish or cancel the Download Assist queue before signing in."
              : undefined
          }
          onClick={onModioSignIn}
        >
          Log in to mod.io
        </button>
        {settings?.has_modio_api_key && (
          <button
            type="button"
            className="danger"
            onClick={onClearModioApiKey}
          >
            Clear mod.io API key
          </button>
        )}
      </div>

      <details style={{ marginTop: "1rem" }}>
        <summary style={{ cursor: "pointer" }}>
          Advanced: Paste mod.io API key manually
        </summary>
        <p style={{ marginTop: "0.75rem" }}>
          Optional fallback if automatic key retrieval fails. Paste your
          read-only API key from{" "}
          <a href="https://mod.io/me/access" target="_blank" rel="noreferrer">
            mod.io → Access
          </a>
          .
        </p>
        <div className="row">
          <input
            type="password"
            placeholder="mod.io API key"
            value={modioKeyInput}
            onChange={(e) => onModioKeyInput(e.target.value)}
          />
          <button onClick={onSaveModioApiKey} disabled={!modioKeyInput.trim()}>
            Save & validate
          </button>
        </div>
      </details>
    </>
  );
}
