import { describe, expect, it } from "vitest";
import { applyIniEdits, isIniFilename, parseIniDocument } from "./iniConfig";

describe("iniConfig", () => {
  it("parses Seamless Co-op ersc_settings.ini sections, comments, and required password", () => {
    const sample = [
      "[GAMEPLAY]",
      "; Allow invaders to join your session",
      "allow_invaders = 1",
      "",
      "[PASSWORD]",
      "; Session password required to connect",
      "cooppassword = ",
    ].join("\r\n");

    const sections = parseIniDocument(sample);
    expect(sections).toHaveLength(2);
    expect(sections[0].name).toBe("GAMEPLAY");
    expect(sections[0].fields[0]).toMatchObject({
      key: "allow_invaders",
      value: "1",
      comments: ["Allow invaders to join your session"],
      requiredWhenEmpty: false,
    });
    expect(sections[1].name).toBe("PASSWORD");
    expect(sections[1].fields[0]).toMatchObject({
      key: "cooppassword",
      value: "",
      requiredWhenEmpty: true,
    });

    const updated = applyIniEdits(sample, {
      [sections[1].fields[0].lineIndex]: "tarnished",
    });
    expect(updated).toContain("cooppassword = tarnished");
    expect(updated).toContain("; Session password required to connect");
    expect(updated).toContain("\r\n");
  });

  it("detects ini filenames case-insensitively", () => {
    expect(isIniFilename("ersc_settings.ini")).toBe(true);
    expect(isIniFilename("CONFIG.INI")).toBe(true);
    expect(isIniFilename("config.toml")).toBe(false);
  });
});
