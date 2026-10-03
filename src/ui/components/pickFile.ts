// Нативный выбор файлов через tauri-plugin-dialog (D10): ядру передаётся путь,
// выбранный пользователем в системном диалоге, а не введённый вручную текст.
import { open, save } from "@tauri-apps/plugin-dialog";

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

/**
 * Путь к архиву модпака (DISC-A02): Modrinth (.mrpack), CurseForge/MultiMC
 * (.zip) или null, если пользователь отменил выбор. Ошибку диалога не глотаем.
 */
export async function pickArchivePath(): Promise<string | null> {
  const picked = await open({
    multiple: false,
    filters: [{ name: "Modrinth/CurseForge/MultiMC архив", extensions: ["zip", "mrpack"] }],
  });
  return typeof picked === "string" ? picked : null;
}

/**
 * Путь для СОХРАНЕНИЯ JSON-файла (экспорт настроек, F20) или null при отмене.
 * Плагин сам спрашивает перезапись существующего файла.
 */
export async function pickSavePath(defaultName: string): Promise<string | null> {
  const picked = await save({
    defaultPath: defaultName,
    filters: [{ name: "JSON", extensions: ["json"] }],
  });
  return typeof picked === "string" ? picked : null;
}

/** Путь к JSON-файлу (импорт настроек, F20) или null при отмене. */
export async function pickJsonPath(): Promise<string | null> {
  const picked = await open({
    multiple: false,
    filters: [{ name: "JSON", extensions: ["json"] }],
  });
  return typeof picked === "string" ? picked : null;
}

/** PNG для логотипа лаунчера (D38) или null при отмене выбора. */
export async function pickPngPath(): Promise<string | null> {
  const picked = await open({
    multiple: false,
    filters: [{ name: "PNG", extensions: ["png"] }],
  });
  return typeof picked === "string" ? picked : null;
}
