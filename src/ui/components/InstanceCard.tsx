// Карточка инстанса: иконка, версия, лоадер, статус, Play (спека §8).
import { FolderOpen, Copy, Trash2, Play, Square, Sparkles } from "lucide-react";
import { t } from "../../i18n";
import type { Instance } from "../../api/types";
import type { LaunchStatus } from "../../state/instances";

export default function InstanceCard({
  inst,
  status,
  onLaunch,
  onStop,
  onOptimize,
  onFolder,
  onDuplicate,
  onDelete,
}: {
  inst: Instance;
  status?: LaunchStatus;
  onLaunch: () => void;
  onStop: () => void;
  onOptimize: () => void;
  onFolder: () => void;
  onDuplicate: () => void;
  onDelete: () => void;
}) {
  const phase = status?.phase;
  const busy = phase !== undefined && phase !== "exited";
  const statusText = phase
    ? phase === "downloading"
      ? t("play.downloading", {
          done: status?.doneFiles ?? 0,
          total: status?.totalFiles ?? 0,
        })
      : phase === "exited"
        ? phase
        : t("play.preparing")
    : null;

  return (
    <div className="flex flex-col rounded-lg border border-border-app bg-card p-4 transition-colors hover:bg-card-hover">
      <div className="flex h-14 w-14 items-center justify-center rounded-md bg-bg text-2xl font-bold text-accent">
        {inst.name.slice(0, 1).toUpperCase()}
      </div>
      <h3 className="mt-3 truncate text-base font-semibold" title={inst.name}>
        {inst.name}
      </h3>
      <p className="text-sm text-text-muted">
        {inst.mcVersion}
        {inst.loader ? ` · ${inst.loader} ${inst.loaderVersion ?? ""}` : ""}
      </p>
      <div className="mt-1 min-h-5 text-xs text-warning" aria-live="polite">
        {statusText ?? ""}
      </div>
      <div className="mt-3 flex items-center gap-2">
        {busy ? (
          <button
            onClick={onStop}
            aria-label={t("instances.stop")}
            className="flex h-10 flex-1 items-center justify-center gap-2 rounded-md bg-error font-semibold text-white transition-colors hover:opacity-90"
          >
            <Square size={15} aria-hidden />
            {t("instances.stop")}
          </button>
        ) : (
          <button
            onClick={onLaunch}
            aria-label={t("instances.launch")}
            className="flex h-10 flex-1 items-center justify-center gap-2 rounded-md bg-accent font-semibold text-accent-contrast transition-colors hover:bg-accent-hover"
          >
            <Play size={15} aria-hidden />
            {t("play")}
          </button>
        )}
        <IconButton icon={Sparkles} label={t("instances.optimize")} onClick={onOptimize} />
        <IconButton icon={FolderOpen} label={t("instances.folder")} onClick={onFolder} />
        <IconButton icon={Copy} label={t("instances.duplicate")} onClick={onDuplicate} />
        <IconButton icon={Trash2} label={t("instances.delete")} onClick={onDelete} danger />
      </div>
    </div>
  );
}

function IconButton({
  icon: Icon,
  label,
  onClick,
  danger,
}: {
  icon: typeof FolderOpen;
  label: string;
  onClick: () => void;
  danger?: boolean;
}) {
  return (
    <button
      onClick={onClick}
      title={label}
      aria-label={label}
      className={`flex h-10 w-10 items-center justify-center rounded-md text-text-muted transition-colors hover:bg-bg ${
        danger ? "hover:text-error" : "hover:text-text"
      }`}
    >
      <Icon size={16} aria-hidden />
    </button>
  );
}
