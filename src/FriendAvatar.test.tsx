import { createRoot } from "react-dom/client";
import { act } from "react";
import { describe, expect, it } from "vitest";
import { FriendAvatar, isSafeAvatarSrc } from "./FriendAvatar";

describe("FriendAvatar security validation", () => {
  it("accepts valid raster base64 data URLs", () => {
    expect(isSafeAvatarSrc("data:image/png;base64,iVBORw0KGgo=")).toBe(true);
    expect(isSafeAvatarSrc("data:image/jpeg;base64,/9j/4AAQ")).toBe(true);
    expect(isSafeAvatarSrc("data:image/webp;base64,UklGRh4=")).toBe(true);
    expect(isSafeAvatarSrc("data:image/gif;base64,R0lGODlh")).toBe(true);
  });

  it("rejects SVG, asset://, external URLs, and oversized payloads", () => {
    expect(isSafeAvatarSrc("data:image/svg+xml;base64,PHN2Zz4=")).toBe(false);
    expect(isSafeAvatarSrc("asset://localhost/home/user/.ssh/id_rsa")).toBe(false);
    expect(isSafeAvatarSrc("https://attacker.example/avatar.png")).toBe(false);
    expect(isSafeAvatarSrc("javascript:alert(1)")).toBe(false);
    expect(
      isSafeAvatarSrc(`data:image/png;base64,${"A".repeat(262_145)}`),
    ).toBe(false);
  });

  it("renders fallback initials instead of img for unsafe src", async () => {
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);
    try {
      await act(async () => {
        root.render(
          <FriendAvatar name="Alice Smith" src="data:image/svg+xml;base64,PHN2Zz4=" />,
        );
      });
      expect(container.querySelector("img")).toBeNull();
      expect(container.textContent).toContain("AS");
    } finally {
      await act(async () => {
        root.unmount();
      });
      container.remove();
    }
  });
});
