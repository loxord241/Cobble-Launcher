// Браузер контента Modrinth (спека §8): поиск, бесконечный скролл, установка
// в инстанс, вкладки типов (моды/шейдеры/ресурспаки/модпаки), установка
// .mrpack, проверка/применение обновлений и управление установленным
// (вкл/выкл, удаление). Паттерн строк «Сланца».
import { useCallback, useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  ChevronDown,
  ChevronUp,
  Download,
  RefreshCw,
  Search,
  SearchX,
  PackageSearch,
  Trash2,
} from "lucide-react";
import { api } from "../../api/client";
import { apiErrorText, currentLanguage, t } from "../../i18n";
import type { ContentEntry, Instance, SearchHit, UpdateCheck } from "../../api/types";

const PAGE_SIZE = 20;

type Tab = "mod" | "shader" | "resourcepack" | "modpack";

const TABS: { id: Tab; defaultQuery: string }[] = [
  { id: "mod", defaultQuery: "mods" },
  { id: "shader", defaultQuery: "shaders" },
  { id: "resourcepack", defaultQuery: "resourcepacks" },
  { id: "modpack", defaultQuery: "modpacks" },
];

function fileBase(file: string): string {
  const base = file.slice(file.lastIndexOf("/") + 1);
  return base.endsWith(".disabled") ? base.slice(0, -".disabled".length) : base;
}

export default function ModsPage({ query }: { query: string }) {
  const [tab, setTab] = useState<Tab>("mod");
  const [localQuery, setLocalQuery] = useState("");
  const [hits, setHits] = useState<SearchHit[]>([]);
  const [total, setTotal] = useState(0);
  const [searched, setSearched] = useState(false);
  const [instances, setInstances] = useState<Instance[]>([]);
  const [targetId, setTargetId] = useState("");
  const [busy, setBusy] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [installedInfo, setInstalledInfo] = useState<string | null>(null);
  const [updates, setUpdates] = useState<UpdateCheck[] | null>(null);
  const [installed, setInstalled] = useState<ContentEntry[]>([]);
  const [showInstalled, setShowInstalled] = useState(false);
  const sentinelRef = useRef<HTMLDivElement>(null);

  // D17: список инстансов мог устареть (инстанс создан/удалён вне страницы),
  // поэтому перечитываем его при монтировании, при возврате фокуса в окно и при
  // появлении вкладки; выбранный targetId сохраняем, если он ещё существует.
  const refreshInstances = useCallback(async () => {
    try {
      const list = await api.instanceList();
      setInstances(list);
      setTargetId((cur) => (list.some((i) => i.id === cur) ? cur : (list[0]?.id ?? "")));
    } catch (e) {
      setError(apiErrorText(e));
    }
  }, []);

  useEffect(() => {
    void refreshInstances();
    const onFocus = () => void refreshInstances();
    const onVisible = () => {
      if (document.visibilityState === "visible") void refreshInstances();
    };
    window.addEventListener("focus", onFocus);
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      window.removeEventListener("focus", onFocus);
      document.removeEventListener("visibilitychange", onVisible);
    };
  }, [refreshInstances]);

  const loadInstalled = useCallback(async () => {
    if (!targetId) return;
    try {
      setInstalled(await api.contentInstalled(targetId));
    } catch {
      setInstalled([]);
    }
  }, [targetId]);

  useEffect(() => {
    void loadInstalled();
  }, [loadInstalled, installedInfo]);

  const doSearch = async () => {
    setBusy(true);
    setError(null);
    try {
      const target = instances.find((i) => i.id === targetId);
      const r = await api.modrinthSearch(
        localQuery || TABS.find((x) => x.id === tab)!.defaultQuery,
        target?.mcVersion,
        target?.loader,
        tab,
        PAGE_SIZE,
        0,
      );
      setHits(r.hits);
      setTotal(r.total_hits);
      setSearched(true);
    } catch (e) {
      setError(apiErrorText(e));
    } finally {
      setBusy(false);
    }
  };

  // Смена вкладки — повторный поиск по выбранному типу контента.
  useEffect(() => {
    if (searched) void doSearch();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tab]);

  // Дозагрузка следующей страницы (offset = сколько уже загрузили).
  const loadMore = useCallback(async () => {
    if (loadingMore || busy || !searched || hits.length >= total) return;
    setLoadingMore(true);
    setError(null);
    try {
      const target = instances.find((i) => i.id === targetId);
      const r = await api.modrinthSearch(
        localQuery || TABS.find((x) => x.id === tab)!.defaultQuery,
        target?.mcVersion,
        target?.loader,
        tab,
        PAGE_SIZE,
        hits.length,
      );
      const known = new Set(hits.map((h) => h.project_id));
      setHits((prev) => [...prev, ...r.hits.filter((h) => !known.has(h.project_id))]);
      setTotal(r.total_hits);
    } catch (e) {
      setError(apiErrorText(e));
    } finally {
      setLoadingMore(false);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [loadingMore, busy, searched, hits, total, instances, targetId, localQuery, tab]);

  // Автопоиск при первом открытии каталога + бесконечный скролл по sentinel.
  useEffect(() => {
    if (!searched) void doSearch();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    const el = sentinelRef.current;
    if (!el) return;
    const root = el.closest("main");
    const io = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) void loadMore();
      },
      { root, rootMargin: "1500px" },
    );
    io.observe(el);
    return () => io.disconnect();
  }, [loadMore]);

  const install = async (projectId: string) => {
    setBusy(true);
    setError(null);
    setInstalledInfo(null);
    try {
      if (tab === "modpack") {
        // Модпак ставится в НОВЫЙ инстанс (ядро: latest → .mrpack → установщик).
        const inst = await api.modpackInstall(projectId);
        setInstances(await api.instanceList());
        setTargetId(inst.id);
        setInstalledInfo(t("mods.packInstalled", { name: inst.name, version: inst.mcVersion }));
      } else {
        const entries = await api.contentInstall(targetId, projectId);
        setInstalledInfo(
          t("mods.installedInto", {
            name: instances.find((i) => i.id === targetId)?.name ?? "?",
            files: entries.map((e) => e.file).join(", "),
          }),
        );
      }
    } catch (e) {
      setError(apiErrorText(e));
    } finally {
      setBusy(false);
    }
  };

  // D5a: файл .mrpack выбираем нативным диалогом (путь вручную не вводим).
  const installMrpack = async () => {
    setBusy(true);
    setError(null);
    try {
      const path = await open({
        multiple: false,
        filters: [{ name: "Modrinth Modpack", extensions: ["mrpack"] }],
      });
      if (typeof path !== "string") return; // отмена диалога
      const inst = await api.mrpackInstall(path);
      setInstalledInfo(t("mods.packInstalled", { name: inst.name, version: inst.mcVersion }));
      setInstances(await api.instanceList());
    } catch (e) {
      setError(apiErrorText(e));
    } finally {
      setBusy(false);
    }
  };

  const checkUpdates = async () => {
    setBusy(true);
    setError(null);
    try {
      setUpdates(await api.contentUpdateCheck(targetId));
    } catch (e) {
      setError(apiErrorText(e));
    } finally {
      setBusy(false);
    }
  };

  const applyUpdates = async () => {
    setBusy(true);
    try {
      const n = await api.contentUpdateAll(targetId);
      setInstalledInfo(t("mods.updatedCount", { n }));
      setUpdates(null);
    } catch (e) {
      setError(apiErrorText(e));
    } finally {
      setBusy(false);
    }
  };

  const toggleEntry = async (file: string) => {
    setError(null);
    try {
      const e = await api.contentToggle(targetId, file);
      setInstalledInfo(t(e.enabled ? "mods.switchedOn" : "mods.switchedOff", { file }));
    } catch (e) {
      setError(apiErrorText(e));
    }
  };

  const removeEntry = async (file: string) => {
    setError(null);
    try {
      await api.contentRemove(targetId, file);
      setInstalledInfo(t("mods.removed", { file }));
    } catch (e) {
      setError(apiErrorText(e));
    }
  };

  // Глобальный поиск из титлбара фильтрует результаты по подстроке.
  const q = query.trim().toLowerCase();
  const visibleHits = hits.filter(
    (h) => !q || h.title.toLowerCase().includes(q) || h.author.toLowerCase().includes(q),
  );
  const target = instances.find((i) => i.id === targetId);

  return (
    <div className="mx-auto flex max-w-[980px] flex-col gap-4">
      <div className="mt-1 flex items-baseline gap-2">
        <h2 className="text-lg font-semibold tracking-tight">{t("nav.mods")}</h2>
        {searched && <span className="chip-mono">{t("mods.shown", { loaded: hits.length, total })}</span>}
      </div>

      {/* Вкладки типов контента (как в Modrinth App). */}
      <div role="tablist" aria-label={t("mods.tabs.label")} className="flex gap-1">
        {TABS.map((x) => (
          <button
            key={x.id}
            role="tab"
            aria-selected={tab === x.id}
            onClick={() => setTab(x.id)}
            className={`h-9 rounded-lg px-3 text-sm font-medium transition-colors ${
              tab === x.id
                ? "bg-accent text-on-accent"
                : "text-text-muted hover:bg-surface-2 hover:text-text"
            }`}
          >
            {t(`mods.tab.${x.id}`)}
          </button>
        ))}
      </div>

      <div className="flex flex-wrap items-center gap-2">
        <div className="flex h-11 min-w-72 flex-1 max-w-[460px] items-center gap-2 rounded-lg border border-border-strong bg-card px-3 text-text-muted">
          <Search size={16} aria-hidden />
          <input
            value={localQuery}
            onChange={(e) => setLocalQuery(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && void doSearch()}
            placeholder={t("mods.searchPlaceholder")}
            className="w-full bg-transparent font-mono text-sm text-text outline-none placeholder:text-text-muted placeholder:font-ui"
            aria-label={t("mods.searchPlaceholder")}
          />
        </div>
        <select
          value={targetId}
          onChange={(e) => {
            setTargetId(e.target.value);
            setUpdates(null);
          }}
          disabled={busy}
          aria-label={t("mods.targetInstance")}
          className="h-11 min-w-40 max-w-[280px] rounded-lg border border-border-strong bg-card px-3 text-sm disabled:cursor-not-allowed disabled:opacity-40"
        >
          {instances.map((i) => (
            <option key={i.id} value={i.id}>
              {i.name} [{i.mcVersion}]
            </option>
          ))}
        </select>
        <button onClick={() => void doSearch()} disabled={busy} className="btn-primary btn-sm">
          <Search size={15} aria-hidden />
          {t("mods.search")}
        </button>
        <button onClick={() => void installMrpack()} disabled={busy} className="btn-ghost btn-sm">
          {t("mods.installMrpack")}
        </button>
        <button
          onClick={() => void checkUpdates()}
          disabled={busy || !targetId}
          className="btn-ghost btn-sm"
        >
          <RefreshCw size={14} aria-hidden />
          {t("mods.checkUpdates")}
        </button>
      </div>

      {error && (
        <p className="rounded-lg border border-error/40 bg-error/10 p-3 text-sm text-error" role="alert">
          {error}
        </p>
      )}
      {installedInfo && (
        <p className="rounded-lg border border-success/40 bg-success/10 p-3 text-sm text-success">
          {installedInfo}
        </p>
      )}

      {/* Установленное в целевом инстансе: вкл/выкл и удаление. */}
      <div className="rounded-lg border border-border-app bg-card">
        <button
          onClick={() => setShowInstalled((v) => !v)}
          aria-expanded={showInstalled}
          className="flex h-11 w-full items-center gap-2 px-3 text-sm font-semibold"
        >
          {showInstalled ? <ChevronUp size={16} aria-hidden /> : <ChevronDown size={16} aria-hidden />}
          {t("mods.installed")}
          <span className="chip-mono">{installed.length}</span>
          {target && <span className="truncate text-text-muted">· {target.name}</span>}
        </button>
        {showInstalled && (
          <div className="border-t border-border-app p-2">
            {installed.length === 0 ? (
              <p className="px-2 py-3 text-[13px] text-text-muted">
                {t("mods.installed.empty", { name: target?.name ?? "?" })}
              </p>
            ) : (
              <ul className="flex flex-col gap-1">
                {installed.map((e) => (
                  <li
                    key={e.file}
                    className={`grid min-h-10 grid-cols-[1fr_auto_auto_auto] items-center gap-2 rounded-md px-2 ${
                      e.enabled ? "" : "opacity-55"
                    }`}
                  >
                    <span className="truncate font-mono text-[13px]" title={e.file}>
                      {fileBase(e.file)}
                    </span>
                    <span className="badge-soft">{t(`mods.kind.${e.kind}`)}</span>
                    <button
                      role="switch"
                      aria-checked={e.enabled}
                      aria-label={t(e.enabled ? "mods.installed.disable" : "mods.installed.enable", { file: fileBase(e.file) })}
                      onClick={() => void toggleEntry(e.file)}
                      className={`h-6 w-10 rounded-full border border-border-strong transition-colors ${
                        e.enabled ? "bg-accent" : "bg-surface-2"
                      }`}
                    >
                      <span
                        aria-hidden
                        className={`block size-4 rounded-full bg-on-accent transition-transform ${
                          e.enabled ? "translate-x-5" : "translate-x-1"
                        }`}
                      />
                    </button>
                    <button
                      aria-label={t("mods.installed.delete", { file: fileBase(e.file) })}
                      onClick={() => void removeEntry(e.file)}
                      className="icon-btn text-error"
                    >
                      <Trash2 size={15} aria-hidden />
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </div>
        )}
      </div>

      {updates !== null && (
        <div className="rounded-lg border border-border-app bg-card p-3">
          <div className="flex items-center justify-between">
            <span className="text-sm font-semibold">{t("mods.updatesFound", { n: updates.length })}</span>
            {updates.length > 0 && (
              <button onClick={() => void applyUpdates()} disabled={busy} className="btn-primary btn-sm">
                <RefreshCw size={14} aria-hidden />
                {t("mods.applyUpdates")}
              </button>
            )}
          </div>
          <ul className="mt-2 space-y-1 text-sm text-text-muted">
            {updates.map((u) => (
              <li key={u.file}>
                {u.file}: {u.latestVersionNumber}
              </li>
            ))}
          </ul>
        </div>
      )}

      {searched && visibleHits.length === 0 ? (
        // Пустое состояние: Modrinth ответил, но по фильтру ничего нет.
        <div className="mt-10 flex flex-col items-center gap-3 rounded-lg border border-border-app bg-card px-4 py-10 text-center">
          <SearchX size={32} aria-hidden className="text-text-muted" />
          <div className="text-[15px] font-semibold">{t("mods.empty.title")}</div>
          <div className="max-w-sm text-[13px] text-text-muted">{t("mods.empty.desc")}</div>
        </div>
      ) : (
        <div className="flex flex-col gap-2">
          {visibleHits.map((h) => (
            <div
              key={h.project_id}
              className="grid min-h-14 grid-cols-[36px_1fr_auto] items-center gap-3 rounded-lg border border-border-app bg-card px-3 py-2 transition-colors hover:border-border-strong hover:bg-card-hover"
            >
              {h.icon_url ? (
                <img
                  src={h.icon_url}
                  alt=""
                  className="size-9 rounded-md border border-border-app object-cover"
                />
              ) : (
                <span
                  aria-hidden
                  className="grid size-9 place-items-center rounded-md bg-surface-2 text-sm font-semibold text-text-muted"
                >
                  {h.title.slice(0, 1)}
                </span>
              )}
              <div className="min-w-0">
                <div className="truncate text-[15px] font-semibold leading-tight">{h.title}</div>
                <div className="font-mono text-[13px] text-text-muted">
                  {t("mods.by", { author: h.author })} · {t("mods.downloads", { n: h.downloads.toLocaleString(currentLanguage()) })}
                </div>
              </div>
              <button
                onClick={() => void install(h.project_id)}
                disabled={busy}
                className="btn-ghost btn-sm"
                aria-label={`${tab === "modpack" ? t("mods.installPack") : t("mods.installInto")} — ${h.title}`}
              >
                <Download size={14} aria-hidden />
                {tab === "modpack" ? t("mods.installPack") : t("mods.installInto")}
              </button>
            </div>
          ))}
          {!searched && (
            // Поиск ещё не выполняли — подсказка вместо пустого списка.
            <div className="mt-10 flex flex-col items-center gap-3 rounded-lg border border-dashed border-border-strong px-4 py-10 text-center">
              <PackageSearch size={32} aria-hidden className="text-text-muted" />
              <div className="max-w-sm text-[13px] text-text-muted">{t("mods.searchPlaceholder")}</div>
            </div>
          )}
          {/* Бесконечный скролл: sentinel догружает страницы, кнопка — для клавиатуры. */}
          {hits.length < total && (
            <div ref={sentinelRef} className="flex justify-center py-2">
              <button
                onClick={() => void loadMore()}
                disabled={loadingMore}
                className="btn-ghost btn-sm"
              >
                {loadingMore ? t("mods.loadingMore") : t("mods.loadMore")}
              </button>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
