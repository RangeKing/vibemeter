import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import { enUS, zhCN } from "./resources";

const initialLocale = navigator.language.toLowerCase().startsWith("zh") ? "zh-CN" : "en-US";

void i18n.use(initReactI18next).init({
  resources: {
    "en-US": { translation: enUS },
    "zh-CN": { translation: zhCN },
  },
  lng: initialLocale,
  fallbackLng: "en-US",
  interpolation: { escapeValue: false },
  returnNull: false,
});

export default i18n;
