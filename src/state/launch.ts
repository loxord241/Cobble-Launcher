// F11: у инстанса может быть свой профиль запуска (accountId). Хелпер резолвит
// имя игрока для запуска: привязанный профиль активируется ТОЧНО так же, как
// кнопка Play на странице инстансов (InstancesPage::onLaunch) — тем же путём
// setActive (IPC + перечитка сторов). Профиль удалён — запускаем от глобального
// активного, как раньше. D8-стиль: живёт вне React, сторы читает через getState().
import { useInstances } from "./instances";
import { useAccounts } from "./accounts";
import { useSettings } from "./settings";

/** Имя игрока для запуска инстанса; при необходимости активирует его профиль. */
export async function resolveLaunchTarget(instanceId: string): Promise<string> {
  const inst = useInstances.getState().list.find((i) => i.id === instanceId);
  const { list: accounts } = useAccounts.getState();
  // Привязанный профиль; удалённого из ядра (нет в списке) считаем отсутствующим.
  const perInst =
    inst?.accountId !== undefined
      ? accounts.find((a) => a.id === inst.accountId)
      : undefined;
  const { settings } = useSettings.getState();
  if (perInst && settings?.accountsActiveId !== perInst.id) {
    // Тот же путь, что в InstancesPage: ядро + перечитка списков (setActive).
    await useAccounts.getState().setActive(perInst.id);
  }
  const activeAccount =
    accounts.find((a) => a.id === settings?.accountsActiveId) ?? accounts[0] ?? null;
  return perInst?.name ?? activeAccount?.name ?? "Player";
}
