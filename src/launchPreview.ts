import type { GlobalLaunchOptions, LaunchCommandSource, LaunchEnvVar } from "./types";

export type ParsedLaunchCommand = {
  envVars: LaunchEnvVar[];
  wrappers: string[];
  args: string[];
};

const ENV_NAME_RE = /^[A-Za-z_][A-Za-z0-9_]*$/;

function splitShellTokens(input: string): string[] {
  const tokens: string[] = [];
  let current = "";
  let quote: '"' | "'" | null = null;
  for (let i = 0; i < input.length; i += 1) {
    const c = input[i];
    if (quote) {
      if (c === "\\" && quote === '"' && i + 1 < input.length) {
        const next = input[i + 1];
        if (next === '"' || next === "\\") {
          current += c + next;
          i += 1;
          continue;
        }
      }
      if (c === quote) {
        quote = null;
      }
      current += c;
    } else if (c === '"' || c === "'") {
      quote = c;
      current += c;
    } else if (/\s/.test(c)) {
      if (current) {
        tokens.push(current);
        current = "";
      }
    } else {
      current += c;
    }
  }
  if (current) tokens.push(current);
  return tokens;
}

function unquoteEnvValue(raw: string): string {
  const trimmed = raw.trim();
  if (
    trimmed.length >= 2 &&
    ((trimmed.startsWith('"') && trimmed.endsWith('"')) ||
      (trimmed.startsWith("'") && trimmed.endsWith("'")))
  ) {
    const inner = trimmed.slice(1, -1);
    if (trimmed.startsWith('"')) {
      return inner.replace(/\\"/g, '"').replace(/\\\\/g, "\\");
    }
    return inner;
  }
  return trimmed;
}

function parseEnvAssignment(token: string): LaunchEnvVar | null {
  const eq = token.indexOf("=");
  if (eq <= 0) return null;
  const name = token.slice(0, eq);
  if (!ENV_NAME_RE.test(name)) return null;
  return { name, value: unquoteEnvValue(token.slice(eq + 1)) };
}

export function parseRawLaunchCommand(raw: string): ParsedLaunchCommand {
  const out: ParsedLaunchCommand = { envVars: [], wrappers: [], args: [] };
  const trimmed = raw.trim();
  if (!trimmed) return out;
  const tokens = splitShellTokens(trimmed);
  const cmdIdx = tokens.indexOf("%command%");

  if (cmdIdx >= 0) {
    let inEnv = true;
    for (const tok of tokens.slice(0, cmdIdx)) {
      if (inEnv) {
        const env = parseEnvAssignment(tok);
        if (env) {
          out.envVars.push(env);
          continue;
        }
        inEnv = false;
      }
      out.wrappers.push(tok);
    }
    for (const tok of tokens.slice(cmdIdx + 1)) {
      if (tok !== "%command%") out.args.push(tok);
    }
  } else {
    let inEnv = true;
    for (const tok of tokens) {
      if (inEnv) {
        const env = parseEnvAssignment(tok);
        if (env) {
          out.envVars.push(env);
          continue;
        }
        inEnv = false;
      }
      if (tok.startsWith("-")) {
        out.args.push(tok);
      } else {
        out.wrappers.push(tok);
      }
    }
  }
  return out;
}

export function mergeWineDllOverrides(existing: string, incoming: string): string {
  const entries: Array<[string, string]> = [];
  for (const source of [existing, incoming]) {
    for (const rawGroup of source.split(";")) {
      const group = rawGroup.trim();
      if (!group) continue;
      const eq = group.indexOf("=");
      const dllList = eq >= 0 ? group.slice(0, eq).trim() : group;
      const mode = eq >= 0 ? group.slice(eq + 1).trim() : "";
      for (const rawDll of dllList.split(",")) {
        const dll = rawDll.trim();
        if (!dll) continue;
        const pos = entries.findIndex(([d]) => d.toLowerCase() === dll.toLowerCase());
        if (pos >= 0) {
          entries[pos][1] = mode;
        } else {
          entries.push([dll, mode]);
        }
      }
    }
  }
  if (entries.length === 0) return "";
  const modeOrder: string[] = [];
  for (const [, mode] of entries) {
    if (!modeOrder.includes(mode)) modeOrder.push(mode);
  }
  return modeOrder
    .map((mode) => {
      const dlls = entries.filter(([, m]) => m === mode).map(([d]) => d);
      return mode ? `${dlls.join(",")}=${mode}` : dlls.join(",");
    })
    .join(";");
}

function stripEmperorToolsBlock(opts: string): string {
  let out = opts;
  while (true) {
    const start = out.indexOf("EMM_TOOLS_BEGIN");
    if (start < 0) break;
    const relEnd = out.slice(start).indexOf("EMM_TOOLS_END");
    if (relEnd < 0) break;
    out = out.slice(0, start) + out.slice(start + relEnd + "EMM_TOOLS_END".length);
  }
  return out.trim().replace(/\s+/g, " ");
}

function pushEnvVar(target: LaunchEnvVar[], incoming: LaunchEnvVar) {
  const name = incoming.name.trim();
  if (!name) return;
  if (name.toUpperCase() === "WINEDLLOVERRIDES") {
    const pos = target.findIndex((v) => v.name.toUpperCase() === "WINEDLLOVERRIDES");
    if (pos >= 0) {
      target[pos] = {
        name: "WINEDLLOVERRIDES",
        value: mergeWineDllOverrides(target[pos].value, incoming.value),
      };
    } else {
      target.push({
        name: "WINEDLLOVERRIDES",
        value: mergeWineDllOverrides("", incoming.value),
      });
    }
    return;
  }
  const pos = target.findIndex((v) => v.name === name);
  if (pos >= 0) {
    target[pos] = { name, value: incoming.value };
  } else {
    target.push({ name, value: incoming.value });
  }
}

function pushUniqueToken(target: string[], token: string) {
  const trimmed = token.trim();
  if (trimmed && !target.includes(trimmed)) {
    target.push(trimmed);
  }
}

function formatEnvVar(v: LaunchEnvVar): string {
  if (/[\s;'"]/.test(v.value)) {
    const escaped = v.value.replace(/\\/g, "\\\\").replace(/"/g, '\\"');
    return `${v.name}="${escaped}"`;
  }
  return `${v.name}=${v.value}`;
}

export function composeLiveLaunchCommand(params: {
  source: LaunchCommandSource;
  launcherOptions?: string | null;
  autoEnvVars: LaunchEnvVar[];
  globalOptions?: GlobalLaunchOptions | null;
  useGlobal: boolean;
  customEnvVars: LaunchEnvVar[];
  customWrapper: string;
  customLaunchArgs: string;
  customRawCommand: string;
  commandToken?: string;
}): string {
  const {
    source,
    launcherOptions,
    autoEnvVars,
    globalOptions,
    useGlobal,
    customEnvVars,
    customWrapper,
    customLaunchArgs,
    customRawCommand,
    commandToken = "%command%",
  } = params;

  if (source === "launcher") {
    const trimmed = launcherOptions?.trim();
    return trimmed || "%command%";
  }

  const envVars: LaunchEnvVar[] = [];
  const wrappers: string[] = [];
  const args: string[] = [];

  if (source === "merged" && launcherOptions) {
    const parsed = parseRawLaunchCommand(stripEmperorToolsBlock(launcherOptions));
    for (const v of parsed.envVars) pushEnvVar(envVars, v);
    for (const w of parsed.wrappers) pushUniqueToken(wrappers, w);
    for (const a of parsed.args) pushUniqueToken(args, a);
  }

  for (const v of autoEnvVars) pushEnvVar(envVars, v);

  if (globalOptions?.enabled && useGlobal) {
    if (globalOptions.raw_command) {
      const parsed = parseRawLaunchCommand(globalOptions.raw_command);
      for (const v of parsed.envVars) pushEnvVar(envVars, v);
      for (const w of parsed.wrappers) pushUniqueToken(wrappers, w);
      for (const a of parsed.args) pushUniqueToken(args, a);
    }
    for (const v of globalOptions.env_vars ?? []) pushEnvVar(envVars, v);
    if (globalOptions.wrapper) {
      for (const tok of splitShellTokens(globalOptions.wrapper)) pushUniqueToken(wrappers, tok);
    }
    if (globalOptions.launch_args) {
      for (const tok of splitShellTokens(globalOptions.launch_args)) pushUniqueToken(args, tok);
    }
  }

  if (customRawCommand.trim()) {
    const parsed = parseRawLaunchCommand(customRawCommand);
    for (const v of parsed.envVars) pushEnvVar(envVars, v);
    for (const w of parsed.wrappers) pushUniqueToken(wrappers, w);
    for (const a of parsed.args) pushUniqueToken(args, a);
  }
  for (const v of customEnvVars) pushEnvVar(envVars, v);
  if (customWrapper.trim()) {
    for (const tok of splitShellTokens(customWrapper)) pushUniqueToken(wrappers, tok);
  }
  if (customLaunchArgs.trim()) {
    for (const tok of splitShellTokens(customLaunchArgs)) pushUniqueToken(args, tok);
  }

  return [
    ...envVars.map(formatEnvVar),
    ...wrappers,
    commandToken,
    ...args,
  ].join(" ");
}
