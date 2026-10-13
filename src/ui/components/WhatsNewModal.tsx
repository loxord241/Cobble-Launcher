// D70: модалка «Что нового» после автообновления. Показывается один раз на
// версию: ключ localStorage "changelogSeenVersion" хранит последнюю увиденную
// версию; он отличается от текущей → показ. D72-хвост: «первый запуск» (ключа
// нет) различается маркером "changelogFreshProfile", который ставит App при
// первом детекте онбординга — у обновившихся со старых версий маркера нет,
// и окно обязано показаться, иначе обновление молча глотало changelog.
// Список изменений — src/changelog.json по текущему языку (фолбэк en → ru);
// записи для версии нет — не открываемся.
import { useCallback, useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { X } from "lucide-react";
import { t, currentLanguage } from "../../i18n";
import { useModalA11y } from "../hooks/useModalA11y";
import rawChangelog from "../../changelog.json";

/** Форма записи changelog.json (тексты правит оркестратор — структура фиксирована). */
interface ChangelogEntry {
  date: string;
  ru: string[];
  en: string[];
  uk: string[];
  pl: string[];
}
type Changelog = Record<string, ChangelogEntry>;

const changelog = rawChangelog as Changelog;
const SEEN_KEY = "changelogSeenVersion";
// Маркер свежей установки (ставит App при первом детекте онбординга).
const FRESH_KEY = "changelogFreshProfile";

/** Пункты для текущего языка с фолбэком en → ru; пусто — показывать нечего. */
function itemsFor(version: string): string[] {
  const entry = changelog[version];
  if (!entry) return [];
  const byLang = entry as unknown as Record<string, string[] | undefined>;
  return byLang[currentLanguage()] ?? entry.en ?? entry.ru ?? [];
}

/** Читаем ключ «увиденной версии»; сбой хранилища — как «ключа нет». */
function readSeen(): string | null {
  try {
    return localStorage.getItem(SEEN_KEY);
  } catch {
    return null;
  }
}

function writeSeen(version: string) {
  try {
    localStorage.setItem(SEEN_KEY, version);
  } catch {
    /* приватный режим — модалка просто покажется снова при следующем запуске */
  }
}

/**
 * Точка монтирования из App: сама решает, показываться или нет (App остаётся
 * тонким). Гейт «настройки загружены и onboardingDone» — на стороне App.
 */
export default function WhatsNewModal() {
  // null = ещё решаем / показывать нечего (нет версии, записи или уже видели).
  const [version, setVersion] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    getVersion()
      .then((v) => {
        // Пустая версия (сбой getVersion) — не показываем вовсе; маркер
        // переживёт до следующего запуска (у свежего профиля seen всё ещё
        // нет — решение будет принято заново).
        if (cancelled || !v) return;
        // Маркер свежей установки гасим СРАЗУ после чтения, до любых ранних
        // выходов: иначе при пустой записи changelog он зависал навсегда и
        // глотал окно СЛЕДУЮЩЕЙ версии (D73-ревизия).
        let fresh = false;
        try {
          fresh = localStorage.getItem(FRESH_KEY) === "1";
          if (fresh) localStorage.removeItem(FRESH_KEY);
        } catch {
          /* приватный режим — считаем профиль обновившимся */
        }
        const seen = readSeen();
        if (seen === v) return; // эту версию уже показывали
        if (itemsFor(v).length === 0) return; // записи в changelog нет
        if (seen === null) {
          if (fresh) {
            // Свежая установка: окно не нужно — всё и так новое.
            writeSeen(v);
            return;
          }
          // Ключа нет, маркера тоже: профиль жил до появления трекинга
          // (обновление со старой версии) — показываем честно.
        }
        setVersion(v);
      })
      .catch(() => {
        /* вне Tauri или сбой IPC — просто не показываем */
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const close = useCallback((v: string) => {
    writeSeen(v);
    setVersion(null);
  }, []);

  if (!version) return null;
  return <WhatsNewDialog version={version} onClose={() => close(version)} />;
}

function WhatsNewDialog({ version, onClose }: { version: string; onClose: () => void }) {
  // A33/D41: Escape, ловушка фокуса, закрытие с анимацией — единый хук модалок.
  const { containerRef: dialogRef, requestClose } = useModalA11y(onClose);
  const items = itemsFor(version);
  const title = t("whatsNew.title", { version });

  return (
    <div
      className="anim-fade-in fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4"
      onClick={(e) => {
        if (e.target === e.currentTarget) requestClose();
      }}
    >
      {/* D41: ref и anim-классы на карточке, не на оверлее (паттерн RenameInstanceModal). */}
      <div
        ref={dialogRef}
        tabIndex={-1}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        className="anim-dialog-in w-[420px] rounded-lg border border-border-app bg-card p-5 shadow-xl"
      >
        <div className="flex items-center justify-between border-b border-border-app pb-3">
          <h2 className="text-lg font-semibold text-text">{title}</h2>
          <button
            onClick={requestClose}
            aria-label={t("common.close")}
            className="icon-btn text-text-muted hover:text-text"
          >
            <X size={18} aria-hidden />
          </button>
        </div>

        <ul className="mt-4 flex flex-col gap-2.5 text-sm text-text">
          {items.map((item, i) => (
            <li key={i} className="flex gap-2.5">
              <span aria-hidden className="mt-[7px] h-1.5 w-1.5 shrink-0 rounded-full bg-accent" />
              <span>{item}</span>
            </li>
          ))}
        </ul>

        <div className="mt-5 flex justify-end">
          <button onClick={requestClose} className="btn-primary h-10 px-5 text-sm">
            {t("whatsNew.continue")}
          </button>
        </div>
      </div>
    </div>
  );
}
