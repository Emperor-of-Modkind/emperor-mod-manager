#!/usr/bin/env node
/**
 * Build an Android APK for Emperor Mod Manager (Tauri v2 Mobile).
 *
 * Handles:
 *  - Android SDK / NDK / Java preflight detection (auto-detects ~/Android/Sdk & latest NDK)
 *  - Installing missing Rust Android targets via rustup
 *  - Running `tauri android init` on first run
 *  - Syncing the Jetpack WindowManager FoldablePlugin.kt, AndroidManifest permissions,
 *    and minSdk (29) / androidx.window dependency into `src-tauri/gen/android`
 *  - Building the APK via `tauri android build --apk` and reporting output paths
 *
 * Usage:
 *   node scripts/build-android-apk.mjs [options]
 *
 * Options:
 *   --debug                 Build a debug APK instead of a release APK
 *   --target <arch>         Target arch: aarch64 (default), armv7, x86_64, i686
 *   --all-targets           Build universal APK across all Android ABIs
 *   --skip-rustup           Do not auto-install missing Rust targets
 *   --help, -h              Show this help message
 */
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import process from "node:process";

const ROOT = path.resolve(import.meta.dirname, "..");
const SRC_TAURI = path.join(ROOT, "src-tauri");
const GEN_ANDROID = path.join(SRC_TAURI, "gen", "android");
const FOLDABLE_PLUGIN_SRC = path.join(
  SRC_TAURI,
  "android-plugin",
  "FoldablePlugin.kt",
);
const FOLDABLE_PLUGIN_DEST = path.join(
  GEN_ANDROID,
  "app",
  "src",
  "main",
  "java",
  "dev",
  "emperormodmanager",
  "mobile",
  "FoldablePlugin.kt",
);
const UPDATE_PLUGIN_SRC = path.join(
  SRC_TAURI,
  "android-plugin",
  "AppUpdatePlugin.kt",
);
const UPDATE_PLUGIN_DEST = path.join(
  GEN_ANDROID,
  "app",
  "src",
  "main",
  "java",
  "dev",
  "emperormodmanager",
  "mobile",
  "AppUpdatePlugin.kt",
);
const FILE_PATHS_SRC = path.join(
  SRC_TAURI,
  "android-plugin",
  "file_paths.xml",
);
const FILE_PATHS_DEST = path.join(
  GEN_ANDROID,
  "app",
  "src",
  "main",
  "res",
  "xml",
  "file_paths.xml",
);

const RUST_TARGETS = {
  aarch64: "aarch64-linux-android",
  armv7: "armv7-linux-androideabi",
  x86_64: "x86_64-linux-android",
  i686: "i686-linux-android",
};

function parseArgs(argv) {
  const opts = {
    debug: false,
    target: "aarch64",
    allTargets: false,
    skipRustup: false,
    help: false,
  };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === "--debug") {
      opts.debug = true;
    } else if (arg === "--all-targets") {
      opts.allTargets = true;
    } else if (arg === "--skip-rustup") {
      opts.skipRustup = true;
    } else if (arg === "--help" || arg === "-h") {
      opts.help = true;
    } else if (arg === "--target") {
      const next = argv[++i];
      if (!next || !(next in RUST_TARGETS)) {
        fail(
          `Invalid --target "${next ?? ""}". Valid values: ${Object.keys(RUST_TARGETS).join(", ")}`,
        );
      }
      opts.target = next;
    } else {
      fail(`Unknown argument: ${arg}. Run with --help for usage.`);
    }
  }
  return opts;
}

function log(msg) {
  console.log(`[build:android] ${msg}`);
}

function fail(msg) {
  console.error(`[build:android] ERROR: ${msg}`);
  process.exit(1);
}

function run(cmd, args, env = process.env) {
  log(`$ ${cmd} ${args.join(" ")}`);
  const res = spawnSync(cmd, args, {
    cwd: ROOT,
    stdio: "inherit",
    shell: process.platform === "win32",
    env,
  });
  if (res.status !== 0) {
    process.exit(res.status ?? 1);
  }
}

function detectAndroidEnv() {
  const env = { ...process.env };

  // Resolve ANDROID_HOME
  if (!env.ANDROID_HOME) {
    const candidates = [
      env.ANDROID_SDK_ROOT,
      path.join(os.homedir(), "Android", "Sdk"),
      path.join(os.homedir(), "Library", "Android", "sdk"),
      "/opt/android-sdk",
    ].filter(Boolean);
    for (const dir of candidates) {
      if (fs.existsSync(dir)) {
        env.ANDROID_HOME = dir;
        break;
      }
    }
  }
  if (!env.ANDROID_HOME || !fs.existsSync(env.ANDROID_HOME)) {
    fail(
      "ANDROID_HOME is not set and no Android SDK was found in ~/Android/Sdk or /opt/android-sdk.\n" +
        "Install Android SDK (Command-Line Tools + Platform 35 + Build-Tools + NDK) and set ANDROID_HOME.",
    );
  }
  env.ANDROID_SDK_ROOT = env.ANDROID_HOME;

  // Resolve NDK_HOME (pick highest version in $ANDROID_HOME/ndk if unset)
  if (!env.NDK_HOME || !fs.existsSync(env.NDK_HOME)) {
    const ndkRoot = path.join(env.ANDROID_HOME, "ndk");
    if (fs.existsSync(ndkRoot)) {
      const versions = fs
        .readdirSync(ndkRoot)
        .filter((d) => fs.statSync(path.join(ndkRoot, d)).isDirectory())
        .sort()
        .reverse();
      if (versions.length > 0) {
        env.NDK_HOME = path.join(ndkRoot, versions[0]);
      }
    }
  }
  if (!env.NDK_HOME || !fs.existsSync(env.NDK_HOME)) {
    fail(
      `Android NDK not found under ${path.join(env.ANDROID_HOME, "ndk")}.\n` +
        "Install NDK via sdkmanager (e.g. `sdkmanager \"ndk;27.2.12479018\"`) and set NDK_HOME.",
    );
  }

  // Verify Java 17+
  const javaCheck = spawnSync("java", ["-version"], {
    encoding: "utf8",
    env,
  });
  if (javaCheck.status !== 0) {
    fail(
      "Java (JDK 17+) was not found on PATH. Install OpenJDK 17+ and ensure JAVA_HOME / PATH are configured.",
    );
  }

  log(`ANDROID_HOME=${env.ANDROID_HOME}`);
  log(`NDK_HOME=${env.NDK_HOME}`);
  return env;
}

function ensureRustTargets(opts, env) {
  if (opts.skipRustup) return;
  const needed = opts.allTargets
    ? Object.values(RUST_TARGETS)
    : [RUST_TARGETS[opts.target]];

  const installedRes = spawnSync("rustup", ["target", "list", "--installed"], {
    encoding: "utf8",
    env,
  });
  if (installedRes.status !== 0) {
    fail("Failed to query `rustup target list --installed`.");
  }
  const installed = new Set(
    installedRes.stdout
      .split(/\r?\n/)
      .map((s) => s.trim())
      .filter(Boolean),
  );
  const missing = needed.filter((t) => !installed.has(t));
  if (missing.length > 0) {
    log(`Installing missing Rust Android targets: ${missing.join(", ")}`);
    run("rustup", ["target", "add", ...missing], env);
  }
}

function ensureAndroidInit(env) {
  if (!fs.existsSync(GEN_ANDROID)) {
    log("Initializing Tauri Android project (`src-tauri/gen/android`)...");
    run("npx", ["tauri", "android", "init", "--ci"], env);
  }
}

function syncFoldablePluginAndConfig() {
  // 1. Sync FoldablePlugin.kt, AppUpdatePlugin.kt, and FileProvider file_paths.xml
  for (const [src, dest, label] of [
    [FOLDABLE_PLUGIN_SRC, FOLDABLE_PLUGIN_DEST, "FoldablePlugin.kt"],
    [UPDATE_PLUGIN_SRC, UPDATE_PLUGIN_DEST, "AppUpdatePlugin.kt"],
    [FILE_PATHS_SRC, FILE_PATHS_DEST, "file_paths.xml"],
  ]) {
    if (fs.existsSync(src)) {
      fs.mkdirSync(path.dirname(dest), { recursive: true });
      fs.copyFileSync(src, dest);
      log(`Synced ${label} -> ${path.relative(ROOT, dest)}`);
    }
  }

  // 2. Ensure AndroidManifest.xml has LAN discovery, network, and APK install permissions + FileProvider
  const manifestPath = path.join(
    GEN_ANDROID,
    "app",
    "src",
    "main",
    "AndroidManifest.xml",
  );
  if (fs.existsSync(manifestPath)) {
    let xml = fs.readFileSync(manifestPath, "utf8");
    const requiredPermissions = [
      "android.permission.INTERNET",
      "android.permission.ACCESS_NETWORK_STATE",
      "android.permission.ACCESS_WIFI_STATE",
      "android.permission.CHANGE_WIFI_MULTICAST_STATE",
      "android.permission.REQUEST_INSTALL_PACKAGES",
    ];
    let changed = false;
    for (const perm of requiredPermissions) {
      if (!xml.includes(perm)) {
        xml = xml.replace(
          "<application",
          `    <uses-permission android:name="${perm}" />\n    <application`,
        );
        changed = true;
      }
    }
    if (!xml.includes(".fileprovider")) {
      const providerBlock = `        <provider
            android:name="androidx.core.content.FileProvider"
            android:authorities="\${applicationId}.fileprovider"
            android:exported="false"
            android:grantUriPermissions="true">
            <meta-data
                android:name="android.support.FILE_PROVIDER_PATHS"
                android:resource="@xml/file_paths" />
        </provider>\n    </application>`;
      xml = xml.replace("</application>", providerBlock);
      changed = true;
    }
    if (changed) {
      fs.writeFileSync(manifestPath, xml, "utf8");
      log("Updated AndroidManifest.xml with permissions and FileProvider.");
    }
  }

  // 3. Ensure build.gradle.kts has minSdk = 29 and androidx.window:window:1.3.0
  const gradlePath = path.join(GEN_ANDROID, "app", "build.gradle.kts");
  if (fs.existsSync(gradlePath)) {
    let gradle = fs.readFileSync(gradlePath, "utf8");
    let changed = false;

    const minSdkMatch = gradle.match(/minSdk\s*=\s*(\d+)/);
    if (minSdkMatch && Number(minSdkMatch[1]) < 29) {
      gradle = gradle.replace(/minSdk\s*=\s*\d+/, "minSdk = 29");
      changed = true;
    }

    if (!gradle.includes("androidx.window:window")) {
      gradle = gradle.replace(
        /dependencies\s*\{/,
        'dependencies {\n    implementation("androidx.window:window:1.3.0")',
      );
      changed = true;
    }

    if (changed) {
      fs.writeFileSync(gradlePath, gradle, "utf8");
      log("Updated app/build.gradle.kts (minSdk = 29, androidx.window:window:1.3.0).");
    }
  }
}

function findApks(dir) {
  const results = [];
  if (!fs.existsSync(dir)) return results;
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      results.push(...findApks(full));
    } else if (entry.name.endsWith(".apk")) {
      results.push(full);
    }
  }
  return results;
}

function signUnsignedApks(apkRoot, env) {
  const btRoot = path.join(env.ANDROID_HOME, "build-tools");
  if (!fs.existsSync(btRoot)) return;
  const btVersions = fs
    .readdirSync(btRoot)
    .filter((d) => fs.statSync(path.join(btRoot, d)).isDirectory())
    .sort()
    .reverse();
  if (btVersions.length === 0) return;

  const btDir = path.join(btRoot, btVersions[0]);
  const apksigner = path.join(btDir, process.platform === "win32" ? "apksigner.bat" : "apksigner");
  const zipalign = path.join(btDir, process.platform === "win32" ? "zipalign.exe" : "zipalign");
  if (!fs.existsSync(apksigner)) return;

  const unsigned = findApks(apkRoot).filter((p) => p.endsWith("-unsigned.apk"));
  if (unsigned.length === 0) return;

  const ksPath = env.ANDROID_KEYSTORE || path.join(os.homedir(), ".android", "emperor-local.keystore");
  const ksPass = env.ANDROID_KEYSTORE_PASSWORD || "emperor-local-dev";
  const ksAlias = env.ANDROID_KEY_ALIAS || "emperor";
  const keyPass = env.ANDROID_KEY_PASSWORD || ksPass;

  if (!fs.existsSync(ksPath)) {
    fs.mkdirSync(path.dirname(ksPath), { recursive: true });
    log(`Generating persistent local signing keystore at ${ksPath}...`);
    run(
      "keytool",
      [
        "-genkeypair",
        "-v",
        "-keystore",
        ksPath,
        "-storepass",
        ksPass,
        "-keypass",
        keyPass,
        "-alias",
        ksAlias,
        "-keyalg",
        "RSA",
        "-keysize",
        "2048",
        "-validity",
        "10000",
        "-dname",
        "CN=Emperor Mod Manager, OU=Dev, O=Emperor, L=Local, ST=Local, C=US",
      ],
      env,
    );
  }

  for (const unsignedApk of unsigned) {
    const alignedApk = unsignedApk.replace(/-unsigned\.apk$/, "-aligned.apk");
    const signedApk = unsignedApk.replace(/-unsigned\.apk$/, "-signed.apk");
    if (fs.existsSync(zipalign)) {
      run(zipalign, ["-p", "-f", "4", unsignedApk, alignedApk], env);
    } else {
      fs.copyFileSync(unsignedApk, alignedApk);
    }
    run(
      apksigner,
      [
        "sign",
        "--ks",
        ksPath,
        "--ks-pass",
        `pass:${ksPass}`,
        "--ks-key-alias",
        ksAlias,
        "--key-pass",
        `pass:${keyPass}`,
        "--out",
        signedApk,
        alignedApk,
      ],
      env,
    );
    fs.rmSync(alignedApk, { force: true });
    log(`Signed APK -> ${path.relative(ROOT, signedApk)}`);
  }
}

function main() {
  const opts = parseArgs(process.argv.slice(2));
  if (opts.help) {
    console.log(`Usage: node scripts/build-android-apk.mjs [options]

Options:
  --debug                 Build a debug APK instead of a release APK
  --target <arch>         Target arch: aarch64 (default), armv7, x86_64, i686
  --all-targets           Build universal APK across all Android ABIs
  --skip-rustup           Do not auto-install missing Rust targets
  --help, -h              Show this help message`);
    process.exit(0);
  }

  const env = detectAndroidEnv();
  ensureRustTargets(opts, env);
  ensureAndroidInit(env);
  syncFoldablePluginAndConfig();

  const tauriArgs = ["tauri", "android", "build", "--apk"];
  if (opts.debug) {
    tauriArgs.push("--debug");
  }
  if (!opts.allTargets) {
    tauriArgs.push("--target", opts.target);
  }

  run("npx", tauriArgs, env);

  const apkRoot = path.join(GEN_ANDROID, "app", "build", "outputs", "apk");
  signUnsignedApks(apkRoot, env);

  let apks = findApks(apkRoot).filter((p) => !p.endsWith("-unsigned.apk"));
  if (apks.length === 0) apks = findApks(apkRoot);
  if (apks.length === 0) {
    log(`Build finished. Check ${path.relative(ROOT, apkRoot)} for output APKs.`);
  } else {
    log("Installable Android APK(s):");
    for (const apk of apks) {
      const mb = (fs.statSync(apk).size / (1024 * 1024)).toFixed(2);
      log(`  - ${path.relative(ROOT, apk)} (${mb} MB)`);
    }
  }
}

main();
