import { describe, expect, it } from "vitest";
import { composeLiveLaunchCommand, mergeWineDllOverrides } from "./launchPreview";

describe("launchPreview", () => {
  it("merges WINEDLLOVERRIDES without dropping auto-detected DLLs", () => {
    expect(mergeWineDllOverrides("dinput8=n,b", "winmm=n,b")).toBe("dinput8,winmm=n,b");
    expect(mergeWineDllOverrides("dinput8=n,b", "version=n")).toBe("dinput8=n,b;version=n");
  });

  it("combines auto, global, and custom add-ons in emperor mode", () => {
    const cmd = composeLiveLaunchCommand({
      source: "emperor",
      launcherOptions: "STEAM_ONLY=1 %command% --steam",
      autoEnvVars: [{ name: "WINEDLLOVERRIDES", value: "dinput8=n,b" }],
      globalOptions: {
        enabled: true,
        env_vars: [{ name: "MANGOHUD", value: "1" }],
        wrapper: "gamemoderun",
        launch_args: null,
        raw_command: "PROTON_ENABLE_WAYLAND=1 %command%",
      },
      useGlobal: true,
      customEnvVars: [],
      customWrapper: "",
      customLaunchArgs: "--launcher-skip",
      customRawCommand: "DXVK_HUD=fps %command% -dx12",
    });

    expect(cmd).not.toContain("STEAM_ONLY");
    expect(cmd).toContain("WINEDLLOVERRIDES=dinput8=n,b");
    expect(cmd).toContain("PROTON_ENABLE_WAYLAND=1");
    expect(cmd).toContain("MANGOHUD=1");
    expect(cmd).toContain("DXVK_HUD=fps");
    expect(cmd).toContain("gamemoderun %command% -dx12 --launcher-skip");
  });

  it("merges Steam + Emperor options in merged mode", () => {
    const cmd = composeLiveLaunchCommand({
      source: "merged",
      launcherOptions: "PROTON_LOG=1 mangohud %command% -novid",
      autoEnvVars: [{ name: "WINEDLLOVERRIDES", value: "winmm=n,b" }],
      globalOptions: {
        enabled: true,
        env_vars: [],
        wrapper: "gamemoderun",
        launch_args: null,
        raw_command: null,
      },
      useGlobal: true,
      customEnvVars: [],
      customWrapper: "",
      customLaunchArgs: "",
      customRawCommand: "",
    });

    expect(cmd).toBe(
      "PROTON_LOG=1 WINEDLLOVERRIDES=winmm=n,b mangohud gamemoderun %command% -novid",
    );
  });

  it("returns launcher options verbatim in launcher-only mode", () => {
    const cmd = composeLiveLaunchCommand({
      source: "launcher",
      launcherOptions: "PROTON_LOG=1 %command%",
      autoEnvVars: [{ name: "WINEDLLOVERRIDES", value: "dinput8=n,b" }],
      globalOptions: { enabled: true, env_vars: [{ name: "MANGOHUD", value: "1" }] },
      useGlobal: true,
      customEnvVars: [],
      customWrapper: "gamemoderun",
      customLaunchArgs: "",
      customRawCommand: "",
    });
    expect(cmd).toBe("PROTON_LOG=1 %command%");
  });
});
