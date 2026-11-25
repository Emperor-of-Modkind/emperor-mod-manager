export type IniField = {
  lineIndex: number;
  key: string;
  value: string;
  comments: string[];
  requiredWhenEmpty: boolean;
};

export type IniSection = {
  name: string;
  fields: IniField[];
};

const REQUIRED_EMPTY_KEYS = new Set(["cooppassword"]);

export function isIniFilename(name: string): boolean {
  return name.toLowerCase().endsWith(".ini");
}

export function parseIniDocument(text: string): IniSection[] {
  const lines = text.replace(/\r\n/g, "\n").split("\n");
  const sections: IniSection[] = [];
  let current: IniSection = { name: "General", fields: [] };
  let pendingComments: string[] = [];

  for (let i = 0; i < lines.length; i += 1) {
    const trimmed = lines[i].trim();
    if (!trimmed) {
      pendingComments = [];
      continue;
    }
    if (trimmed.startsWith(";") || trimmed.startsWith("#")) {
      const cleaned = trimmed.replace(/^[;#]+\s*/, "").trim();
      if (cleaned) pendingComments.push(cleaned);
      continue;
    }
    const secMatch = trimmed.match(/^\[([^\]]+)\]$/);
    if (secMatch) {
      if (current.fields.length > 0) sections.push(current);
      current = { name: secMatch[1].trim(), fields: [] };
      pendingComments = [];
      continue;
    }
    const eq = lines[i].indexOf("=");
    if (eq > 0) {
      const key = lines[i].slice(0, eq).trim();
      const rawVal = lines[i].slice(eq + 1).trim();
      if (key) {
        current.fields.push({
          lineIndex: i,
          key,
          value: rawVal,
          comments: pendingComments,
          requiredWhenEmpty: REQUIRED_EMPTY_KEYS.has(key.toLowerCase()),
        });
      }
    }
    pendingComments = [];
  }

  if (current.fields.length > 0) sections.push(current);
  return sections;
}

/**
 * Apply edited field values back onto the original INI text, preserving all
 * comments, blank lines, ordering, and CRLF vs LF line endings.
 */
export function applyIniEdits(
  original: string,
  edits: Record<number, string>,
): string {
  const eol = original.includes("\r\n") ? "\r\n" : "\n";
  const lines = original.replace(/\r\n/g, "\n").split("\n");
  for (const [idxStr, nextValue] of Object.entries(edits)) {
    const idx = Number(idxStr);
    if (!Number.isInteger(idx) || idx < 0 || idx >= lines.length) continue;
    const line = lines[idx];
    const eq = line.indexOf("=");
    if (eq <= 0) continue;
    const prefix = line.slice(0, eq).trimEnd();
    const trimmedNext = nextValue.trim();
    lines[idx] = trimmedNext ? `${prefix} = ${trimmedNext}` : `${prefix} =`;
  }
  return lines.join(eol);
}
