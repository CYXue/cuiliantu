import { useTranslation } from "../i18n";
import { GlobeIcon } from "./Icons";

export function LanguageSwitcher() {
  const { i18n, t, lang } = useTranslation();

  const changeLanguage = (code: string) => {
    i18n.changeLanguage(code);
  };

  return (
    <div class="flex items-center gap-2 px-3 py-1.5 rounded-full border border-slate-200 bg-[var(--surface-card)] hover:bg-slate-50 hover:border-slate-300 transition-all duration-200 shadow-sm hover:shadow dark:border-slate-700 dark:hover:border-slate-600">
      <GlobeIcon class="w-4 h-4 text-slate-500" />
      <select
        aria-label={t("language.label")}
        value={lang()}
        onChange={(e) => changeLanguage(e.currentTarget.value)}
        class="bg-transparent text-sm font-medium text-slate-600 hover:text-slate-800 cursor-pointer outline-none"
      >
        <option value="en">{t("language.en")}</option>
        <option value="ja">{t("language.ja")}</option>
        <option value="zh-CN">{t("language.zh-CN")}</option>
      </select>
    </div>
  );
}
