import { useEffect, useState } from "react";
import { X, Trash2, Globe, Shield, User, Plus, ExternalLink } from "lucide-react";
import { useAccounts, accountKindLabelKey } from "../../state/accounts";
import { useSettings } from "../../state/settings";
import { t } from "../../i18n";

type Tab = "msa" | "ely" | "offline";

export default function AccountsModal({ onClose }: { onClose: () => void }) {
  const { list: accounts, setActive, remove, addOffline, addMsaBrowser, addEly } = useAccounts();
  const { settings } = useSettings();
  const [activeTab, setActiveTab] = useState<Tab>("offline");

  // Офлайн форма
  const [offlineNick, setOfflineNick] = useState("");

  // Ely.by форма
  const [elyUser, setElyUser] = useState("");
  const [elyPass, setElyPass] = useState("");

  const [busy, setBusy] = useState(false);
  const [statusMsg, setStatusMsg] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const activeId = settings?.accountsActiveId ?? accounts[0]?.id;

  const handleAddOffline = async (e: React.FormEvent) => {
    e.preventDefault();
    const nick = offlineNick.trim();
    if (!nick) return;
    setBusy(true);
    setError(null);
    try {
      const acc = await addOffline(nick);
      await setActive(acc.id);
      setOfflineNick("");
      setStatusMsg(t("accounts.offlineAdded", { name: nick }));
    } catch (err) {
      setError(String((err as { message?: string })?.message ?? err));
    } finally {
      setBusy(false);
    }
  };

  const handleAddMsa = async () => {
    setBusy(true);
    setError(null);
    setStatusMsg(t("accounts.msaWaiting"));
    try {
      const acc = await addMsaBrowser();
      await setActive(acc.id);
      setStatusMsg(t("accounts.msaSuccess", { name: acc.name }));
    } catch (err) {
      setError(String((err as { message?: string })?.message ?? err));
      setStatusMsg(null);
    } finally {
      setBusy(false);
    }
  };

  const handleAddEly = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!elyUser.trim() || !elyPass) return;
    setBusy(true);
    setError(null);
    try {
      const acc = await addEly(elyUser.trim(), elyPass);
      await setActive(acc.id);
      setElyUser("");
      setElyPass("");
      setStatusMsg(t("accounts.elySuccess", { name: acc.name }));
    } catch (err) {
      setError(String((err as { message?: string })?.message ?? err));
    } finally {
      setBusy(false);
    }
  };

  const handleDelete = async (id: string, name: string) => {
    if (window.confirm(t("accounts.confirmDelete", { name }))) {
      try {
        await remove(id);
      } catch (err) {
        setError(String(err));
      }
    }
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4"
      role="dialog"
      aria-modal="true"
      aria-label={t("accounts.title")}
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="flex max-h-[90vh] w-[620px] max-w-full flex-col rounded-lg border border-border-app bg-card p-6 shadow-2xl">
        <div className="flex items-center justify-between border-b border-border-app pb-3">
          <h2 className="text-lg font-semibold text-text">{t("accounts.title")}</h2>
          <button
            onClick={onClose}
            aria-label={t("common.close")}
            className="icon-btn text-text-muted hover:text-text"
          >
            <X size={18} aria-hidden />
          </button>
        </div>

        {/* Список существующих аккаунтов */}
        <div className="mt-4">
          <div className="text-xs font-semibold uppercase tracking-wider text-text-muted">
            {t("accounts.currentList")} ({accounts.length})
          </div>
          <div className="mt-2 flex max-h-44 flex-col gap-2 overflow-y-auto pr-1">
            {accounts.length === 0 ? (
              <div className="rounded-md border border-border-app bg-bg p-3 text-center text-sm text-text-muted">
                {t("accounts.noAccounts")}
              </div>
            ) : (
              accounts.map((acc) => {
                const isActive = acc.id === activeId;
                return (
                  <div
                    key={acc.id}
                    className={`flex items-center justify-between rounded-lg border px-3 py-2 ${
                      isActive
                        ? "border-accent bg-accent-soft/30"
                        : "border-border-app bg-bg hover:border-border-strong"
                    }`}
                  >
                    <div className="flex items-center gap-3">
                      <span className="grid size-8 place-items-center rounded-md bg-surface-2 text-sm font-bold text-accent">
                        {acc.name.slice(0, 1).toUpperCase()}
                      </span>
                      <div>
                        <div className="flex items-center gap-2">
                          <span className="text-sm font-semibold text-text">{acc.name}</span>
                          {isActive && (
                            <span className="rounded bg-accent px-1.5 py-0.5 text-[11px] font-bold text-on-accent">
                              {t("accounts.active")}
                            </span>
                          )}
                        </div>
                        <span className="text-xs text-text-muted">
                          {t(accountKindLabelKey[acc.kind])}
                        </span>
                      </div>
                    </div>

                    <div className="flex items-center gap-1">
                      {!isActive && (
                        <button
                          onClick={() => void setActive(acc.id)}
                          className="btn-ghost h-8 px-2.5 text-xs"
                        >
                          {t("accounts.select")}
                        </button>
                      )}
                      <button
                        onClick={() => void handleDelete(acc.id, acc.name)}
                        aria-label={t("accounts.delete", { name: acc.name })}
                        className="grid size-8 place-items-center rounded-md text-text-muted hover:bg-error/10 hover:text-error"
                      >
                        <Trash2 size={15} aria-hidden />
                      </button>
                    </div>
                  </div>
                );
              })
            )}
          </div>
        </div>

        {/* Добавление нового аккаунта */}
        <div className="mt-5 border-t border-border-app pt-4">
          <div className="text-xs font-semibold uppercase tracking-wider text-text-muted">
            {t("accounts.addNew")}
          </div>

          <div className="mt-2 flex gap-2 border-b border-border-app pb-2">
            <button
              onClick={() => {
                setActiveTab("offline");
                setError(null);
                setStatusMsg(null);
              }}
              className={`flex h-9 items-center gap-2 rounded-md px-3 text-sm font-medium transition-colors ${
                activeTab === "offline"
                  ? "bg-accent text-on-accent"
                  : "text-text-muted hover:bg-surface-2 hover:text-text"
              }`}
            >
              <User size={15} />
              {t("account.kind.offline")}
            </button>
            <button
              onClick={() => {
                setActiveTab("msa");
                setError(null);
                setStatusMsg(null);
              }}
              className={`flex h-9 items-center gap-2 rounded-md px-3 text-sm font-medium transition-colors ${
                activeTab === "msa"
                  ? "bg-accent text-on-accent"
                  : "text-text-muted hover:bg-surface-2 hover:text-text"
              }`}
            >
              <Globe size={15} />
              Microsoft
            </button>
            <button
              onClick={() => {
                setActiveTab("ely");
                setError(null);
                setStatusMsg(null);
              }}
              className={`flex h-9 items-center gap-2 rounded-md px-3 text-sm font-medium transition-colors ${
                activeTab === "ely"
                  ? "bg-accent text-on-accent"
                  : "text-text-muted hover:bg-surface-2 hover:text-text"
              }`}
            >
              <Shield size={15} />
              Ely.by
            </button>
          </div>

          <div className="mt-3">
            {/* Офлайн форма */}
            {activeTab === "offline" && (
              <form onSubmit={handleAddOffline} className="flex gap-2">
                <input
                  type="text"
                  value={offlineNick}
                  onChange={(e) => setOfflineNick(e.target.value)}
                  placeholder={t("accounts.nickPlaceholder")}
                  className="h-10 flex-1 rounded-md border border-border-app bg-bg px-3 text-sm text-text outline-none focus:border-accent"
                />
                <button
                  type="submit"
                  disabled={busy || !offlineNick.trim()}
                  className="btn-primary flex h-10 items-center gap-2 px-4 text-sm"
                >
                  <Plus size={16} />
                  {t("accounts.add")}
                </button>
              </form>
            )}

            {/* Microsoft форма */}
            {activeTab === "msa" && (
              <div className="flex flex-col gap-3 rounded-lg border border-border-app bg-bg p-4">
                <p className="text-xs leading-relaxed text-text-muted">
                  {t("accounts.msaHint")}
                </p>
                <button
                  type="button"
                  onClick={() => void handleAddMsa()}
                  disabled={busy}
                  className="btn-primary flex h-10 items-center justify-center gap-2 text-sm"
                >
                  <ExternalLink size={16} />
                  {busy ? t("accounts.msaWaiting") : t("accounts.msaLoginBtn")}
                </button>
              </div>
            )}

            {/* Ely.by форма */}
            {activeTab === "ely" && (
              <form onSubmit={handleAddEly} className="flex flex-col gap-2">
                <input
                  type="text"
                  value={elyUser}
                  onChange={(e) => setElyUser(e.target.value)}
                  placeholder={t("accounts.elyUserPlaceholder")}
                  className="h-10 rounded-md border border-border-app bg-bg px-3 text-sm text-text outline-none focus:border-accent"
                />
                <input
                  type="password"
                  value={elyPass}
                  onChange={(e) => setElyPass(e.target.value)}
                  placeholder={t("accounts.elyPassPlaceholder")}
                  className="h-10 rounded-md border border-border-app bg-bg px-3 text-sm text-text outline-none focus:border-accent"
                />
                <button
                  type="submit"
                  disabled={busy || !elyUser.trim() || !elyPass}
                  className="btn-primary mt-1 flex h-10 items-center justify-center gap-2 text-sm"
                >
                  <Plus size={16} />
                  {t("accounts.elyLoginBtn")}
                </button>
              </form>
            )}
          </div>
        </div>

        {statusMsg && (
          <p className="mt-3 rounded border border-accent/40 bg-accent-soft/30 p-2 text-xs text-accent">
            {statusMsg}
          </p>
        )}

        {error && (
          <p className="mt-3 rounded border border-error/40 bg-error/10 p-2 text-xs text-error" role="alert">
            {error}
          </p>
        )}

        <div className="mt-5 flex justify-end border-t border-border-app pt-3">
          <button onClick={onClose} className="btn-primary h-10 px-5 text-sm">
            {t("common.done")}
          </button>
        </div>
      </div>
    </div>
  );
}
