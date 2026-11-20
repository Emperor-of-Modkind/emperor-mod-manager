import { describe, expect, it } from "vitest";
import { nexusModUnavailable, nexusUnavailableNotice } from "./nexusAvailability";

describe("nexusUnavailableNotice", () => {
  it("explains a hidden mod, including the page reason", () => {
    const notice = nexusUnavailableNotice({
      available: false,
      status: "hidden",
      availability_note:
        "This mod has been set to hidden. Hidden at 06 Nov 2026, 7:57PM by FSNovask for the following reason: This mod is currently not supported by the author(s) and/or has issue(s) they are unable to fix yet.",
    });
    expect(notice).toEqual({
      title: "This mod has been set to hidden on Nexus.",
      reason:
        "Hidden at 06 Nov 2026, 7:57PM by FSNovask for the following reason: This mod is currently not supported by the author(s) and/or has issue(s) they are unable to fix yet.",
      body: "Your installed copy is unchanged. Nexus is not sharing the description, files, or updates.",
      files: null,
    });
  });

  it("says Nexus is not listing files on the files tab", () => {
    const notice = nexusUnavailableNotice(
      { available: false, status: "under_moderation" },
      { files: true },
    );
    expect(notice?.title).toBe("This mod has been set to under moderation on Nexus.");
    expect(notice?.files).toBe("Nexus is not listing files for this mod.");
  });

  it("covers a deleted mod that Nexus no longer returns", () => {
    const notice = nexusUnavailableNotice({ available: false, status: null });
    expect(notice?.title).toBe("This mod is no longer available on Nexus.");
    expect(notice?.reason).toBeNull();
  });

  it("stays quiet for a published mod", () => {
    expect(
      nexusModUnavailable({ available: true, status: "published" }),
    ).toBe(false);
    expect(
      nexusUnavailableNotice({ available: true, status: "published" }),
    ).toBeNull();
    expect(nexusUnavailableNotice(null)).toBeNull();
  });
});
