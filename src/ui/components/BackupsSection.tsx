// Настройки → «Бэкапы инстансов» (F4). Самодостаточна, как UpdatesSection:
// без пропсов, вставляется в SettingsPage одной строкой. Верх — политика
// хранения (сколько последних бэкапов инстанса держать; ротацию выполняет
// ядро после каждого нового бэкапа), низ — реестр всех zip из каталога
// бэкапов с удалением по одному. Команды backups_list/backup_delete и поле
// Settings.backupKeep — контракт оркестратора F4.
import { useCallback, useEffect, useState } from "react";
import { Archive, FolderOpen, Trash2 } from "lucide-react";
import { api } from "../../api/client";
import { apiErrorText, currentLanguage, t } from "../../i18n";
import { askConfirm } from "../confirm";
import { formatSize } from "../format";
import type { BackupEntry } from "../../api/types";

/** Пресеты политики хранения; 0 — без ротации (∞, как раньше). */
const KEEP_CHOICES = [1, 3, 5, 10, 0];

export default function BackupsSection() {
  // null — настройки ещё читаются (select показывает дефолт 3).
  const [keep, setKeep] = useState<number | null>(null);
  const [backups, setBackups] = useState<BackupEntry[] | null>(null);
  const [busyFile, setBusyFile] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setBackups(await api.backupsList());
    } catch (e) {
      setError(apiErrorText(e));
    }
  }, []);

  useEffect(() => {
    api
      .settingsGet()
      .then((s) => setKeep(s.backupKeep ?? 3))
      .catch(() => setKeep(3));
    void refresh();
  }, [refresh]);

  /** Смена политики хранения: пишем поле backupKeep целиком через
   * settings_set (перечитываем настройки, чтобы не затереть чужие поля). */
  const changeKeep = async (value: number) => {
    const prev = keep;
    setKeep(value);
    try {
      const current = await api.settingsGet();
      await api.settingsSet({ ...current, backupKeep: value });
      setError(null);
    } catch (e) {
      // D69-ревизия: селект не должен показывать значение, которого нет в ядре.
      setKeep(prev);
      setError(apiErrorText(e));
    }
  };

  /** Удаление одного бэкапа: подтверждение (красное, разрушительное) →
   * backup_delete → перечитываем список. */
  const removeBackup = async (entry: BackupEntry) => {
    const ok = await askConfirm(entry.file, {
      danger: true,
      confirmLabel: t("settings.backups.delete"),
      detail: `${entry.instance} · ${new Date(entry.createdAt * 1000).toLocaleString(currentLanguage())} · ${formatSize(entry.sizeBytes)}`,
    });
    if (!ok) return;
    setBusyFile(entry.file);
    try {
      await api.backupDelete(entry.file);
      setError(null);
      await refresh();
    } catch (e) {
      setError(apiErrorText(e));
    } finally {
      setBusyFile(null);
    }
  };

  return (
    <section>
      <h3 className="mb-3 text-[13px] font-semibold uppercase tracking-wider text-text-muted">
        {t("settings.backups.title")}
      </h3>
      <div className="rounded-lg border border-border-app bg-card p-4">
        {/* Политика хранения: сколько последних бэкапов инстанса держать */}
        <div className="flex min-h-12 items-center justify-between gap-4 py-2">
          <span className="text-sm">{t("settings.backups.keep")}</span>
          <div className="flex flex-col items-end gap-1">
            <select
              value={String(keep ?? 3)}
              onChange={(e) => void changeKeep(Number(e.target.value))}
              aria-label={t("settings.backups.keep")}
              className="h-10 rounded-lg border border-border-strong bg-card px-3 text-sm"
            >
              {KEEP_CHOICES.map((v) => (
                <option key={v} value={v}>
                  {v === 0 ? t("settings.backups.unlimited") : v}
                </option>
              ))}
            </select>
            <span className="text-[13px] text-text-muted">{t("settings.backups.keepHint")}</span>
          </div>
        </div>

        {/* Поломка IPC — красная плашка (как в Shell/UpdatesSection) */}
        {error && (
          <p
            role="alert"
            className="mt-3 rounded-lg border border-error/40 bg-error/10 p-3 text-sm text-error"
          >
            {error}
          </p>
        )}

        {/* Реестр всех бэкапов: инстанс · дата · размер */}
        <div className="mt-4 border-t border-border-app pt-3">
          <div className="mb-2 flex items-center justify-between gap-2">
            <div className="text-xs font-medium text-text-muted">{t("settings.backups.list")}</div>
            <button
              type="button"
              onClick={() => void api.dirOpen("backups").catch((e) => setError(apiErrorText(e)))}
              aria-label={t("settings.backups.open")}
              className="btn-ghost btn-sm flex items-center gap-2"
            >
              <FolderOpen size={14} aria-hidden />
              {t("settings.backups.open")}
            </button>
          </div>

          {backups === null ? null : backups.length === 0 ? (
            <p className="flex items-center gap-2 text-sm text-text-muted">
              <Archive size={14} aria-hidden />
              {t("settings.backups.empty")}
            </p>
          ) : (
            <div className="flex flex-col gap-1">
              {backups.map((b) => (
                <div
                  key={b.file}
                  className="flex items-center gap-2 rounded-md border border-border-app bg-bg px-3 py-2"
                >
                  <Archive size={14} aria-hidden className="shrink-0 text-text-muted" />
                  <span className="min-w-0 truncate text-sm" title={b.file}>
                    {b.instance}
                  </span>
                  <span className="ml-auto flex shrink-0 items-center gap-3 text-xs text-text-muted">
                    <span>
                      {new Date(b.createdAt * 1000).toLocaleString(currentLanguage())}
                    </span>
                    <span className="font-mono">{formatSize(b.sizeBytes)}</span>
                  </span>
                  <button
                    type="button"
                    onClick={() => void removeBackup(b)}
                    disabled={busyFile === b.file}
                    aria-busy={busyFile === b.file}
                    aria-label={`${t("settings.backups.delete")}: ${b.file}`}
                    className="btn-ghost btn-sm shrink-0 text-error"
                  >
                    <Trash2 size={14} aria-hidden />
                    {t("settings.backups.delete")}
                  </button>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>
    </section>
  );
}
