export type NexusAvailabilityDetail = {
  available?: boolean | null;
  status?: string | null;
  availability_note?: string | null;
};

const NON_PUBLIC = [
  "hidden",
  "removed",
  "deleted",
  "under moderation",
  "not published",
  "wastebinned",
];

export function plainNexusStatus(status: string): string {
  return status.trim().replace(/_/g, " ").replace(/\s+/g, " ").toLowerCase();
}

export function nexusModUnavailable(
  detail: NexusAvailabilityDetail | null | undefined,
): boolean {
  if (!detail) return false;
  if (detail.available === false) return true;
  const status = detail.status ? plainNexusStatus(detail.status) : "";
  if (!status) return false;
  return NON_PUBLIC.some((marker) => status === marker || status.includes(marker));
}

export type NexusUnavailableNotice = {
  title: string;
  reason: string | null;
  body: string;
  files: string | null;
};

const INSTALLED_LINE =
  "Your installed copy is unchanged. Nexus is not sharing the description, files, or updates.";

/** Drop a leading "set to hidden" sentence the title already states. */
function reasonBeyondStatus(
  note: string | null | undefined,
  status: string,
): string | null {
  const trimmed = note?.trim() ?? "";
  if (!trimmed) return null;
  const lower = trimmed.toLowerCase();
  if (!lower.startsWith("this mod has been set to ")) return trimmed;
  const split = trimmed.indexOf(". ");
  if (split === -1) return null;
  const first = trimmed.slice(0, split).toLowerCase();
  if (status && !first.includes(status)) return trimmed;
  const rest = trimmed.slice(split + 2).trim();
  return rest || null;
}

export function nexusUnavailableNotice(
  detail: NexusAvailabilityDetail | null | undefined,
  options?: { files?: boolean },
): NexusUnavailableNotice | null {
  if (!nexusModUnavailable(detail)) return null;
  const status = detail?.status ? plainNexusStatus(detail.status) : "";
  const reason = reasonBeyondStatus(detail?.availability_note, status);
  return {
    title: status
      ? `This mod has been set to ${status} on Nexus.`
      : "This mod is no longer available on Nexus.",
    reason,
    body: INSTALLED_LINE,
    files: options?.files ? "Nexus is not listing files for this mod." : null,
  };
}
