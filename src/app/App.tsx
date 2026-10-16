import { QueryClientProvider } from "@tanstack/react-query";
import { MotionConfig } from "motion/react";
import { useEffect, useState } from "react";
import { I18nProvider, resolveLocale } from "../lib/i18n";
import { queryClient, useAppInfo, useBackendEvents, useSettings } from "../lib/queries";
import { isTauri } from "../lib/ipc";
import { ToastProvider } from "../components/ui";
import { NavProvider } from "./nav";
import { Shell } from "./Shell";
import { Onboarding } from "../features/onboarding/Onboarding";
import { BootScreen } from "./BootScreen";

function usePreferredScheme(): "dark" | "light" {
  const get = () =>
    typeof window !== "undefined" && window.matchMedia?.("(prefers-color-scheme: light)").matches ? "light" : "dark";
  const [scheme, setScheme] = useState<"dark" | "light">(get);
  useEffect(() => {
    const mq = window.matchMedia?.("(prefers-color-scheme: light)");
    if (!mq) return;
    const on = () => setScheme(get());
    mq.addEventListener("change", on);
    return () => mq.removeEventListener("change", on);
  }, []);
  return scheme;
}

function Root() {
  useBackendEvents();
  const settings = useSettings();
  const info = useAppInfo();
  const system = usePreferredScheme();
  const pref = settings.data?.theme ?? "system";
  const theme = pref === "system" ? system : pref;
  const locale = resolveLocale(settings.data?.language);
  const [onboardingDone, setOnboardingDone] = useState(false);

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    document.documentElement.lang = locale;
    if (isTauri()) {
      void import("@tauri-apps/api/window")
        .then(({ getCurrentWindow }) => getCurrentWindow().setTheme(theme))
        .catch(() => {});
    }
  }, [theme, locale]);

  const showOnboarding = info.data && !info.data.onboarded && !onboardingDone;

  return (
    <I18nProvider locale={locale}>
      <ToastProvider>
        <NavProvider>
          {!info.data && !info.error ? (
            <BootScreen />
          ) : showOnboarding ? (
            <Onboarding info={info.data!} onDone={() => setOnboardingDone(true)} />
          ) : (
            <Shell />
          )}
        </NavProvider>
      </ToastProvider>
    </I18nProvider>
  );
}

export function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <MotionConfig reducedMotion="user">
        <Root />
      </MotionConfig>
    </QueryClientProvider>
  );
}
