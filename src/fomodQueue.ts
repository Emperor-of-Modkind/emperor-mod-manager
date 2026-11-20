/** One installer pass started from Configure or Deploy. The length stays fixed. */
export type FomodPass<T> = {
  mods: T[];
  index: number;
  deployAfter: boolean;
};

export type FomodPassStep<T> =
  | { action: "open"; pass: FomodPass<T> }
  | { action: "deploy" }
  | { action: "idle" };

export function startFomodPass<T>(mods: T[], deployAfter: boolean): FomodPassStep<T> {
  if (mods.length === 0) {
    return deployAfter ? { action: "deploy" } : { action: "idle" };
  }
  return {
    action: "open",
    pass: { mods, index: 0, deployAfter },
  };
}

export function fomodPassCurrent<T>(pass: FomodPass<T>): T {
  return pass.mods[pass.index];
}

/** 1-based position within the pass that started this dialog. */
export function fomodPassProgress(pass: FomodPass<unknown>): { current: number; total: number } {
  return { current: pass.index + 1, total: pass.mods.length };
}

/** Finish saves the open mod, then opens the next or ends the pass. */
export function finishFomodPass<T>(pass: FomodPass<T>): FomodPassStep<T> {
  const next = pass.index + 1;
  if (next >= pass.mods.length) {
    return pass.deployAfter ? { action: "deploy" } : { action: "idle" };
  }
  return { action: "open", pass: { ...pass, index: next } };
}

/** Cancel, close, and Back end the pass. A deploy-started pass does not deploy. */
export function cancelFomodPass<T>(pass: FomodPass<T>): { action: "idle" } {
  void pass;
  return { action: "idle" };
}

/** Deploy only asks about installers that would be linked. */
export function fomodModsForDeploy<T extends { enabled: boolean }>(mods: T[]): T[] {
  return mods.filter((mod) => mod.enabled);
}
