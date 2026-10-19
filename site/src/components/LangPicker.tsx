import { useHistory } from "@docusaurus/router";
import { useStorageSlot } from "@docusaurus/theme-common";
import { useQueryStringValue } from "@docusaurus/theme-common/internal";
import clsx from "clsx";
import { useEffect, useRef, useState } from "react";
import { LANGS, type Lang } from "../langs.ts";

const KEY = "lang";
const isLang = (v: string | null): v is Lang => v !== null && v in LANGS;

function useLang(): [Lang, (lang: Lang) => void] {
  const history = useHistory();
  const query = useQueryStringValue(KEY);
  const [stored, slot] = useStorageSlot(`docusaurus.tab.${KEY}`);
  const current = isLang(query) ? query : isLang(stored) ? stored : "rust";
  const pick = (lang: Lang) => {
    slot.set(lang);
    if (query !== null) {
      const params = new URLSearchParams(history.location.search);
      params.set(KEY, lang);
      history.replace({ ...history.location, search: params.toString() });
    }
  };
  return [current, pick];
}

const Chevron = () => (
  <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true">
    <path fill="currentColor" d="M3.3 5.8a1 1 0 0 1 1.4 0L8 9.1l3.3-3.3a1 1 0 1 1 1.4 1.4l-4 4a1 1 0 0 1-1.4 0l-4-4a1 1 0 0 1 0-1.4Z" />
  </svg>
);

function useOpen() {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const away = (e: PointerEvent) => {
      if (!ref.current?.contains(e.target as Node)) setOpen(false);
    };
    const esc = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    document.addEventListener("pointerdown", away);
    document.addEventListener("keydown", esc);
    return () => {
      document.removeEventListener("pointerdown", away);
      document.removeEventListener("keydown", esc);
    };
  }, [open]);
  return { open, setOpen, ref };
}

const Glyph = () => (
  <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true">
    <path
      fill="currentColor"
      d="M5.7 4.3a1 1 0 0 1 0 1.4L3.4 8l2.3 2.3a1 1 0 1 1-1.4 1.4l-3-3a1 1 0 0 1 0-1.4l3-3a1 1 0 0 1 1.4 0Zm4.6 0a1 1 0 0 1 1.4 0l3 3a1 1 0 0 1 0 1.4l-3 3a1 1 0 1 1-1.4-1.4L12.6 8l-2.3-2.3a1 1 0 0 1 0-1.4Z"
    />
  </svg>
);

export default function LangPicker({ mobile = false }: { mobile?: boolean }) {
  const [current, pick] = useLang();
  const { open, setOpen, ref } = useOpen();
  const items = (Object.entries(LANGS) as [Lang, (typeof LANGS)[Lang]][]).map(([lang, { label }]) => (
    <li key={lang} className={mobile ? "menu__list-item" : undefined}>
      <button
        type="button"
        className={clsx(mobile ? "menu__link" : "dropdown__link", "lang-option", {
          [mobile ? "menu__link--active" : "dropdown__link--active"]: lang === current,
        })}
        aria-pressed={lang === current}
        onClick={() => {
          pick(lang);
          setOpen(false);
        }}
      >
        {label}
      </button>
    </li>
  ));
  if (mobile) {
    return (
      <li className="menu__list-item lang-picker--mobile">
        <span className="menu__link lang-picker__title">
          <Glyph /> Language
        </span>
        <ul className="menu__list">{items}</ul>
      </li>
    );
  }
  return (
    <div ref={ref} className={clsx("navbar__item dropdown dropdown--hoverable dropdown--right lang-picker", { "dropdown--show": open })}>
      <button
        type="button"
        className="lang-picker__button"
        aria-haspopup="true"
        aria-expanded={open}
        onClick={() => setOpen(!open)}
      >
        <Glyph />
        <span>{LANGS[current].label}</span>
        <Chevron />
      </button>
      <ul className="dropdown__menu">{items}</ul>
    </div>
  );
}
