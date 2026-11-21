import { describe, expect, it } from "vitest";
import { setupCanFinish } from "./setupGate";

describe("setupCanFinish", () => {
  it("enables finish when website sign-in or a personal API key is saved", () => {
    expect(setupCanFinish(true, true)).toBe(true);
    expect(setupCanFinish(true, false)).toBe(true);
    expect(setupCanFinish(false, true)).toBe(true);
    expect(setupCanFinish(false, false)).toBe(false);
  });
});
