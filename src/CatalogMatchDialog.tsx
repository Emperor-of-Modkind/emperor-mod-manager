import { useEffect, useMemo, useState } from "react";

import { Modal } from "./Modal";
import { SourceMark } from "./SourceMark";
import type { CatalogCandidate, CatalogSuggestion, DetectedGame } from "./types";

export type CatalogMatchEngine = "unreal" | "bepinex";

export type CatalogMatchSubmit = {
  nexusDomain: string;
  thunderstoreCommunity: string;
  modioGameId: number | null;
  projectName: string | null;
};

type Props = {
  game: DetectedGame;
  engine: CatalogMatchEngine;
  suggestion: CatalogSuggestion | null;
  lookingUp: boolean;
  projectName?: string;
  busy: boolean;
  onClose: () => void;
  onSubmit: (values: CatalogMatchSubmit) => void;
};

type SourceKey = "nexus" | "thunderstore" | "modio";

type FlatCandidate = CatalogCandidate & { source: SourceKey };

const HIGH_CONFIDENCE = 90;

function flattenCandidates(s: CatalogSuggestion | null): FlatCandidate[] {
  if (!s) {
    return [];
  }
  const rows: FlatCandidate[] = [];
  for (const c of s.nexus_candidates ?? []) {
    rows.push({ ...c, source: "nexus" });
  }
  for (const c of s.thunderstore_candidates ?? []) {
    rows.push({ ...c, source: "thunderstore" });
  }
  for (const c of s.modio_candidates ?? []) {
    rows.push({ ...c, source: "modio" });
  }
  rows.sort((a, b) => b.score - a.score || a.name.length - b.name.length);
  return rows;
}

function confidenceLabel(score: number): string {
  if (score >= HIGH_CONFIDENCE) return "Strong";
  if (score >= 70) return "Close";
  return "Similar";
}

export function CatalogMatchDialog({
  game,
  engine,
  suggestion,
  lookingUp,
  projectName: initialProject = "",
  busy,
  onClose,
  onSubmit,
}: Props) {
  const [domain, setDomain] = useState("");
  const [community, setCommunity] = useState("");
  const [modioId, setModioId] = useState("");
  const [project, setProject] = useState(initialProject);

  useEffect(() => {
    setDomain(suggestion?.nexus_domain ?? game.nexus_domain ?? "");
    setCommunity(suggestion?.thunderstore_community ?? "");
    setModioId(
      suggestion?.modio_game_id != null && suggestion.modio_game_id > 0
        ? String(suggestion.modio_game_id)
        : "",
    );
  }, [suggestion, game.nexus_domain]);

  useEffect(() => {
    setProject(initialProject);
  }, [initialProject]);

  const candidates = useMemo(() => flattenCandidates(suggestion), [suggestion]);
  const hasHit = Boolean(
    domain.trim() ||
      community.trim() ||
      (Number.parseInt(modioId.trim(), 10) > 0),
  );

  function applyCandidate(row: FlatCandidate) {
    if (row.source === "nexus") {
      setDomain(row.id);
    } else if (row.source === "thunderstore") {
      setCommunity(row.id);
    } else {
      setModioId(row.id);
    }
  }

  function submit() {
    const nexusDomain = domain.trim().toLowerCase().replace(/\s+/g, "");
    const thunderstoreCommunity = community.trim().toLowerCase();
    const midRaw = modioId.trim();
    const parsed = midRaw ? Number.parseInt(midRaw, 10) : 0;
    const modioGameId = Number.isFinite(parsed) && parsed > 0 ? parsed : null;
    if (!nexusDomain && !thunderstoreCommunity && modioGameId == null) {
      return;
    }
    onSubmit({
      nexusDomain,
      thunderstoreCommunity,
      modioGameId,
      projectName: engine === "unreal" ? project.trim() || null : null,
    });
  }

  const engineLabel = engine === "unreal" ? "Unreal Engine" : "Unity / BepInEx";
  const subtitle = lookingUp
    ? "Looking up close catalog matches…"
    : candidates.length > 0
      ? suggestion?.ambiguous
        ? "Several close matches — pick the right catalog page, or enter IDs manually."
        : "Suggested catalog pages from the game title. Confirm or edit before managing."
      : "No close catalog page found. Enter a Nexus domain, Thunderstore community, and/or mod.io game ID.";

  return (
    <Modal
      className="modal-catalog-match"
      title={`Match catalog for ${game.title}`}
      subtitle={`${engineLabel} · ${subtitle}`}
      onClose={onClose}
      footer={
        <div className="modal-foot-actions">
          <button type="button" className="linkish" onClick={onClose} disabled={busy}>
            Cancel
          </button>
          <button type="button" disabled={busy || !hasHit || lookingUp} onClick={submit}>
            Manage
          </button>
        </div>
      }
    >
      {lookingUp ? (
        <p className="note">Searching Nexus, Thunderstore, and mod.io…</p>
      ) : null}

      {!lookingUp && candidates.length > 0 ? (
        <div className="catalog-match-suggestions">
          <h3>Suggestions</h3>
          <ul className="list catalog-match-list">
            {candidates.map((row) => {
              const selected =
                (row.source === "nexus" && domain === row.id) ||
                (row.source === "thunderstore" && community === row.id) ||
                (row.source === "modio" && modioId === row.id);
              return (
                <li key={`${row.source}:${row.id}`}>
                  <button
                    type="button"
                    className={
                      selected
                        ? "catalog-match-row catalog-match-row-selected"
                        : "catalog-match-row"
                    }
                    onClick={() => applyCandidate(row)}
                    disabled={busy}
                  >
                    <span className="catalog-match-row-main">
                      <strong>{row.name}</strong>
                      <small>
                        <SourceMark source={row.source} />
                        {` · ${row.id}`}
                      </small>
                    </span>
                    <span className="catalog-match-score">
                      {confidenceLabel(row.score)}
                    </span>
                  </button>
                </li>
              );
            })}
          </ul>
        </div>
      ) : null}

      {!lookingUp && candidates.length === 0 ? (
        <p className="note">
          We detected a moddable {engineLabel} install, but could not match a catalog
          page automatically.
        </p>
      ) : null}

      <div className="catalog-match-fields">
        <h3>Catalog IDs</h3>
        <label>
          Nexus domain (optional)
          <input
            value={domain}
            onChange={(e) => setDomain(e.target.value)}
            placeholder="e.g. palworld"
            disabled={busy}
            autoFocus
          />
          {suggestion?.nexus_name ? (
            <span className="field-hint">Matched: {suggestion.nexus_name}</span>
          ) : null}
        </label>
        <label>
          Thunderstore community (optional)
          <input
            value={community}
            onChange={(e) => setCommunity(e.target.value)}
            placeholder="e.g. palworld"
            disabled={busy}
          />
          {suggestion?.thunderstore_name ? (
            <span className="field-hint">Matched: {suggestion.thunderstore_name}</span>
          ) : null}
        </label>
        <label>
          mod.io game ID (optional)
          <input
            value={modioId}
            onChange={(e) => setModioId(e.target.value)}
            placeholder="e.g. 1234"
            inputMode="numeric"
            disabled={busy}
          />
          {suggestion?.modio_name ? (
            <span className="field-hint">Matched: {suggestion.modio_name}</span>
          ) : null}
        </label>
        {engine === "unreal" ? (
          <label>
            Project folder (optional)
            <input
              value={project}
              onChange={(e) => setProject(e.target.value)}
              placeholder="Auto-detected when possible"
              disabled={busy}
            />
          </label>
        ) : null}
      </div>
    </Modal>
  );
}
