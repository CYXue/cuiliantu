import { For } from "solid-js";
import { useTranslation } from "../i18n";
import { getTheme, setTheme, type ThemeMode } from "../theme";
import { MonitorIcon, MoonIcon, SunIcon } from "./Icons";

const OPTIONS: {
  mode: ThemeMode;
  icon: typeof SunIcon;
  key: string;
}[] = [
  { mode: "light", icon: SunIcon, key: "theme.light" },
  { mode: "dark", icon: MoonIcon, key: "theme.dark" },
  { mode: "system", icon: MonitorIcon, key: "theme.system" },
];

export function ThemeToggle() {
  const { t } = useTranslation();

  return (
    <div class="flex items-center gap-0.5 px-1.5 py-1 rounded-full border border-slate-200 bg-[var(--surface-card)] hover:border-slate-300 transition-all duration-200 shadow-sm hover:shadow dark:border-slate-700">
      <For each={OPTIONS}>
        {(o) => {
          const Icon = o.icon;
          const active = () => getTheme() === o.mode;
          return (
            <button
              type="button"
              onClick={() => setTheme(o.mode)}
              title={t(o.key)}
              aria-label={t(o.key)}
              aria-pressed={active()}
              class={`p-1.5 rounded-full transition-colors duration-150 ${
                active()
                  ? "bg-indigo-50 text-indigo-600 dark:bg-indigo-500/20 dark:text-indigo-300"
                  : "text-slate-500 hover:text-slate-700 dark:text-slate-400 dark:hover:text-slate-200"
              }`}
            >
              <Icon class="w-4 h-4" />
            </button>
          );
        }}
      </For>
    </div>
  );
}
