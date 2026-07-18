import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import { en, type DictKey } from "./en";
import { zh } from "./zh";

export type Lang = "en" | "zh";

const dictionaries: Record<Lang, Record<DictKey, string>> = { en, zh };
const storageKey = "coworker-desktop-lang";

function detectInitialLang(): Lang {
  const stored = window.localStorage.getItem(storageKey);
  if (stored === "en" || stored === "zh") return stored;
  return navigator.language.toLowerCase().startsWith("zh") ? "zh" : "en";
}

function interpolate(template: string, vars?: Record<string, string | number>) {
  if (!vars) return template;
  return template.replace(/\{\{(\w+)\}\}/g, (match, name) => (name in vars ? String(vars[name]) : match));
}

type I18nContextValue = {
  lang: Lang;
  setLang: (lang: Lang) => void;
  t: (key: DictKey, vars?: Record<string, string | number>) => string;
};

const I18nContext = createContext<I18nContextValue | null>(null);

export function LanguageProvider({ children }: { children: ReactNode }) {
  const [lang, setLangState] = useState<Lang>(detectInitialLang);

  useEffect(() => {
    document.documentElement.lang = lang === "zh" ? "zh-CN" : "en";
  }, [lang]);

  const value = useMemo<I18nContextValue>(() => {
    const dict = dictionaries[lang];
    return {
      lang,
      setLang: (nextLang: Lang) => {
        window.localStorage.setItem(storageKey, nextLang);
        setLangState(nextLang);
      },
      t: (key, vars) => interpolate(dict[key] ?? key, vars),
    };
  }, [lang]);

  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

export function useI18n() {
  const context = useContext(I18nContext);
  if (!context) throw new Error("useI18n must be used within a LanguageProvider");
  return context;
}
