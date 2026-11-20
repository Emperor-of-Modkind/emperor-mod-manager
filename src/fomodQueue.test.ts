import { describe, expect, it } from "vitest";

import {
  cancelFomodPass,
  finishFomodPass,
  fomodModsForDeploy,
  fomodPassCurrent,
  fomodPassProgress,
  startFomodPass,
} from "./fomodQueue";

const a = { id: "a", enabled: true };
const b = { id: "b", enabled: true };
const c = { id: "c", enabled: false };

describe("fomod queue", () => {
  it("opens the first mod and keeps list order", () => {
    const step = startFomodPass([a, b, c], false);
    expect(step.action).toBe("open");
    if (step.action !== "open") return;
    expect(step.pass.mods.map((mod) => mod.id)).toEqual(["a", "b", "c"]);
    expect(fomodPassCurrent(step.pass).id).toBe("a");
    expect(fomodPassProgress(step.pass)).toEqual({ current: 1, total: 3 });
  });

  it("advances the counter on finish and stays inside the original total", () => {
    const started = startFomodPass([a, b, c], false);
    expect(started.action).toBe("open");
    if (started.action !== "open") return;

    const second = finishFomodPass(started.pass);
    expect(second.action).toBe("open");
    if (second.action !== "open") return;
    expect(fomodPassCurrent(second.pass).id).toBe("b");
    expect(fomodPassProgress(second.pass)).toEqual({ current: 2, total: 3 });

    const third = finishFomodPass(second.pass);
    expect(third.action).toBe("open");
    if (third.action !== "open") return;
    expect(fomodPassProgress(third.pass)).toEqual({ current: 3, total: 3 });
    expect(finishFomodPass(third.pass)).toEqual({ action: "idle" });
  });

  it("cancel closes the dialog and does not deploy", () => {
    const started = startFomodPass([a, b], true);
    expect(started.action).toBe("open");
    if (started.action !== "open") return;
    expect(cancelFomodPass(started.pass)).toEqual({ action: "idle" });
  });

  it("deploys only after the last finish of a deploy pass", () => {
    const started = startFomodPass([a, b], true);
    expect(started.action).toBe("open");
    if (started.action !== "open") return;

    const second = finishFomodPass(started.pass);
    expect(second.action).toBe("open");
    if (second.action !== "open") return;
    expect(fomodPassProgress(second.pass)).toEqual({ current: 2, total: 2 });
    expect(finishFomodPass(second.pass)).toEqual({ action: "deploy" });
  });

  it("a later pass counts only the mods that still need choices", () => {
    const again = startFomodPass([b, c], false);
    expect(again.action).toBe("open");
    if (again.action !== "open") return;
    expect(again.pass.mods.map((mod) => mod.id)).toEqual(["b", "c"]);
    expect(fomodPassProgress(again.pass)).toEqual({ current: 1, total: 2 });
  });

  it("deploy skips disabled mods and deploys immediately when none remain", () => {
    expect(fomodModsForDeploy([a, b, c]).map((mod) => mod.id)).toEqual(["a", "b"]);
    expect(startFomodPass(fomodModsForDeploy([c]), true)).toEqual({ action: "deploy" });
  });
});
