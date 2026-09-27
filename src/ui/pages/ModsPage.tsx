// Браузер контента Modrinth (спека §8): поиск, бесконечный скролл, установка
// в инстанс, установка .mrpack, проверка/применение обновлений. Паттерн строк «Сланца».
import { useCallback, useEffect, useRef, useState } from "react";
import { Download, RefreshCw, Search, SearchX, PackageSearch } from "lucide-react";
import { api } from "../../api/client";
import { t } from "../../i18n";
import type { Instance, SearchHit, UpdateCheck } from "../../api/types";

const PAGE_SIZE = 20;

export default function ModsPage({ query }: { query: string }) {
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
  const sentinelRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    api.instanceList().then((list) => {
      setInstances(list);
      if (list[0]) setTargetId(list[0].id);
    });
  }, []);

  const doSearch = async () => {
    setBusy(true);
    setError(null);
    try {
      const target = instances.find((i) => i.id === targetId);
      const r = await api.modrinthSearch(
        localQuery || "mods",
        target?.mcVersion,
        target?.loader,
        undefined,
        PAGE_SIZE,
        0,
      );
      setHits(r.hits);
      setTotal(r.total_hits);
      setSearched(true);
    } catch (e) {
      setError((e as { message?: string })?.message ?? String(e));
    } finally {
      setBusy(false);
    }
  };

  // Дозагрузка следующей страницы (offset = сколько уже загрузили).
  const loadMore = useCallback(async () => {
    if (loadingMore || busy || !searched || hits.length >= total) return;
    setLoadingMore(true);
    setError(null);
    try {
      const target = instances.find((i) => i.id === targetId);
      const r = await api.modrinthSearch(
        localQuery || "mods",
        target?.mcVersion,
        target?.loader,
        undefined,
        PAGE_SIZE,
        hits.length,
      );
      const known = new Set(hits.map((h) => h.project_id));
      setHits((prev) => [...prev, ...r.hits.filter((h) => !known.has(h.project_id))]);
      setTotal(r.total_hits);
    } catch (e) {
      setError((e as { message?: string })?.message ?? String(e));
    } finally {
      setLoadingMore(false);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [loadingMore, busy, searched, hits, total, instances, targetId, localQuery]);

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
      const entries = await api.contentInstall(targetId, projectId);
      setInstalledInfo(
        t("mods.installedInto", {
          name: instances.find((i) => i.id === targetId)?.name ?? "?",
          files: entries.map((e) => e.file).join(", "),
        }),
      );
    } catch (e) {
      setError((e as { message?: string })?.message ?? String(e));
    } finally {
      setBusy(false);
    }
  };

  const installMrpack = async () => {
    const path = window.prompt(t("mods.mrpackPrompt"));
    if (!path) return;
    setBusy(true);
    setError(null);
    try {
      const inst = await api.mrpackInstall(path);
      setInstalledInfo(t("mods.packInstalled", { name: inst.name, version: inst.mcVersion }));
      setInstances(await api.instanceList());
    } catch (e) {
      setError((e as { message?: string })?.message ?? String(e));
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
      setError((e as { message?: string })?.message ?? String(e));
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
      setError((e as { message?: string })?.message ?? String(e));
    } finally {
      setBusy(false);
    }
  };

  // Глобальный поиск из титлбара фильтрует результаты по подстроке.
  const q = query.trim().toLowerCase();
  const visibleHits = hits.filter(
    (h) => !q || h.title.toLowerCase().includes(q) || h.author.toLowerCase().includes(q),
  );

  return (
    <div className="mx-auto flex max-w-[980px] flex-col gap-4">
      <div className="mt-1 flex items-baseline gap-2">
        <h2 className="text-lg font-semibold tracking-tight">{t("nav.mods")}</h2>
        {searched && <span className="chip-mono">{t("mods.shown", { loaded: hits.length, total })}</span>}
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
          aria-label={t("mods.targetInstance")}
          className="h-11 min-w-40 max-w-[280px] rounded-lg border border-border-strong bg-card px-3 text-sm"
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

      {updates !== null && (
        <div className="rounded-lg border border-border-app bg-card p-3">
          <div className="flex items-center justify-between">
            <span className="text-sm font-semibold">{t("mods.updatesFound", { n: updates.length })}</span>
            {updates.length > 0 && (
              <button onClick={() => void applyUpdates()} className="btn-primary btn-sm">
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
                  {t("mods.by", { author: h.author })} · {t("mods.downloads", { n: h.downloads.toLocaleString("ru") })}
                </div>
              </div>
              <button
                onClick={() => void install(h.project_id)}
                disabled={busy || !targetId}
                className="btn-ghost btn-sm"
                aria-label={`${t("mods.installInto")} — ${h.title}`}
              >
                <Download size={14} aria-hidden />
                {t("mods.installInto")}
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
