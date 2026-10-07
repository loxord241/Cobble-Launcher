// Обложка пакета в строке списка (F25, D37-B): pack.png из zip как data-URL
// или серый плейсхолдер с иконкой; title — описание из pack.mcmeta. Чистая
// витрина: данные приходит пропсами со страницы (команда pack_art), здесь
// нет ни загрузок, ни запросов — переиспользуется вкладками контента.
import type { ReactNode } from "react";
import { Package } from "lucide-react";

export default function PackArt({
  art,
  children,
}: {
  art?: { png?: string; description?: string };
  children: ReactNode;
}) {
  return (
    <div className="flex min-w-0 items-center gap-3">
      {art?.png ? (
        <img
          src={art.png}
          alt=""
          title={art.description}
          draggable={false}
          loading="lazy"
          className="size-10 shrink-0 rounded object-cover"
        />
      ) : (
        <span
          aria-hidden
          title={art?.description}
          className="grid size-10 shrink-0 place-items-center rounded bg-surface-2 text-text-muted"
        >
          <Package size={18} />
        </span>
      )}
      <div className="min-w-0 flex-1">{children}</div>
    </div>
  );
}
