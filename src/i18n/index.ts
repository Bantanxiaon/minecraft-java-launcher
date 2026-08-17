import zhCN from "./zh-CN.json";
import enUS from "./en-US.json";

export type TranslationKey = keyof typeof zhCN;

const dictionaries: Record<string, Record<TranslationKey, string>> = {
  "zh-CN": zhCN,
  "en-US": enUS,
};

const defaultLocale = "zh-CN";

export function t(key: TranslationKey, locale: string = defaultLocale): string {
  const dictionary = dictionaries[locale] ?? dictionaries[defaultLocale];
  return dictionary[key] ?? key;
}

export function currentLocale(): string {
  try {
    const saved = localStorage.getItem("sh-launcher-locale");
    if (saved && dictionaries[saved]) return saved;
  } catch {
    // 存储不可用时使用默认中文
  }
  return defaultLocale;
}
