import { lazy, Suspense, useEffect, useState } from "react";
import { api } from "./api";
import type { AppInfo } from "./types";
import appLogo from "./assets/app-logo.png";

const DesktopApp = lazy(() => import("./App"));
const MobileApp = lazy(() => import("./MobileApp"));

function BootSplash() {
  return (
    <div className="boot-splash">
      <img src={appLogo} alt="Emperor Mod Manager" className="boot-splash-logo" />
    </div>
  );
}

export function AppRoot() {
  const [appInfo, setAppInfo] = useState<AppInfo | null>(null);

  useEffect(() => {
    let active = true;
    api
      .getAppInfo()
      .then((info) => {
        if (active) setAppInfo(info);
      })
      .catch(() => {
        if (active) {
          setAppInfo({
            name: "emperor-mod-manager",
            version: "0.0.0",
            platform: "unknown",
            is_mobile: false,
            linux_only: false,
          });
        }
      });
    return () => {
      active = false;
    };
  }, []);

  if (!appInfo) {
    return <BootSplash />;
  }

  return (
    <Suspense fallback={<BootSplash />}>
      {appInfo.is_mobile ? <MobileApp appInfo={appInfo} /> : <DesktopApp />}
    </Suspense>
  );
}
