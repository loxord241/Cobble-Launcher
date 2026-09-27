// Нативный выбор файлов через tauri-plugin-dialog (D10): ядру передаётся путь,
// выбранный пользователем в системном диалоге, а не введённый вручную текст.
import { open } from "@tauri-apps/plugin-dialog";

/**
 * Путь к картинке-иконке инстанса (PNG/JPEG/WebP) или null, если пользователь
 * отменил выбор. Ошибку самого диалога не глотаем — вызывающий код показывает её
 * через apiErrorText().
 */
export async function pickImagePath(): Promise<string | null> {
  const picked = await open({
    multiple: false,
    filters: [{ name: "PNG / JPEG / WebP", extensions: ["png", "jpg", "jpeg", "webp"] }],
  });
  return typeof picked === "string" ? picked : null;
}
