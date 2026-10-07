<div align="center">

<sub>[English](README.md) · [Русский](README.ru.md) · <b>Українська</b> · [Polski](README.pl.md)</sub>

<img src="src-tauri/icons/128x128.png" width="88" alt="Cobble Launcher">

# Cobble Launcher

**Чесний лаунчер Minecraft для Windows**

[![Версія](https://img.shields.io/github/v/release/loxord241/Cobble-Launcher?style=flat-square&label=версія)](https://github.com/loxord241/Cobble-Launcher/releases/latest)
[![Ліцензія](https://img.shields.io/github/license/loxord241/Cobble-Launcher?style=flat-square)](LICENSE)
[![Платформа](https://img.shields.io/badge/Windows-10%2F11_x64-blue?style=flat-square&logo=windows)](https://github.com/loxord241/Cobble-Launcher/releases/latest)
[![Мови](https://img.shields.io/badge/мови-ru%C2%B7en%C2%B7uk%C2%B7pl-green?style=flat-square)](#мови)
[![Тести](https://img.shields.io/badge/тести-457%2B10-success?style=flat-square)](#збірка-з-вихідного-коду)

**[Завантажити останню версію](https://github.com/loxord241/Cobble-Launcher/releases/latest)** ·
[Повідомити про проблему](https://github.com/loxord241/Cobble-Launcher/issues) ·
[Демо-відео](docs/anim-demo.mp4)
· [Сайт Cobble Launcher](https://loxord241.github.io/Cobble-Launcher/)

<img src="docs/screenshots/home.png" alt="Каталог Modrinth на головній сторінці" width="820">

Tauri 2 · Rust · React 19 · TypeScript

</div>

---

Cobble — настільний лаунчер із нативним ядром на Rust і легким інтерфейсом на
React. Жодних браузерних рушіїв у фоні, жодних «прискорювачів» та інʼєкцій у
гру: лише офіційні джерела, перевірка кожного файла за хешем і sodium-стек для
оптимізації.

## Каталог Modrinth

- Каталог прямо на головній: модпаки та моди, пошук, сортування
  (завантаження / улюблені / оновлення), фільтр завантажувача, нескінченний
  скрол
- Вікно проєкта як у CurseForge: Огляд / Журнал змін / Галерея / Версії
- Установлення модпаків у новий інстанс і модів у наявний — прямо з вікна
  проєкта або зі сторінки інстансу; пакетне оновлення модів в один клік

## Інстанси

- Ізольовані інстанси: власна версія, завантажувач (Fabric / Quilt / Forge /
  NeoForge), моди та налаштування
- Прогрес завантаження і живий статус прямо на картці; захист від подвійного
  запуску та від псування працюючого інстансу — на рівні ядра
- Повний контроль: RAM, версія Java (автовстановлення Adoptium), пресети
  прапорців JVM, швидкий запуск (Quick Play), профілі на інстанс,
  перейменування, дублювання, резервні копії та снапшоти модів з відкатом
- Світи, скріншоти та конфіги модів — прямо зі сторінки інстансу

## Профілі

- Microsoft (браузерний OAuth), Ely.by (authlib-injector) та офлайн-профілі;
  офлайн — чесно позначені як неліцензійні
- Аватари: голова скіна для Microsoft / Ely.by, класичний Стів — для офлайну
- Refresh-токени — лише в системному keyring Windows
- Discord Rich Presence: назва інстансу у статусі Discord

<div align="center">
  <table>
    <tr>
      <td><img src="docs/screenshots/instances.png" alt="Інстанси: картки з артами та версіями" width="420"></td>
      <td><img src="docs/screenshots/accounts.png" alt="Керування профілями: справжній Стів для офлайн-профілів" width="420"></td>
    </tr>
    <tr>
      <td><img src="docs/screenshots/instance.png" alt="Сторінка інстансу: запуск, вкладки, статистика" width="420"></td>
      <td><img src="docs/screenshots/settings.png" alt="Налаштування: теми, мова, памʼять" width="420"></td>
    </tr>
  </table>
</div>

## Автооновлення

Лаунчер сам перевіряє релізи (раз за сесію, тихо) і показує бейдж «Доступне
оновлення». Встановлення — в один клік: підписане оновлення (minisign), прогрес
завантаження, автоматичний перезапуск. Ваші інстанси, профілі та налаштування
залишаються на місці.

## Безпека і приватність

- **Лише офіційні джерела**: Mojang, Modrinth, Adoptium — кожен файл звіряється
  з хешем із сервера. Дзеркала без перевірки хешів заборонені принципами
  проєкту.
- **Нуль інʼєкцій у гру**: лаунчер не модифікує процес Minecraft. Єдиний
  виняток — authlib-injector для Ely.by, який є частиною цієї служби.
- **Секрети не залишають систему**: токени — у keyring Windows, а в логах
  увімкнено автоматичне приховування секретів.

## Мови

Російська · English · Українська · Polski — обираються в онбордингу або в
налаштуваннях, застосовуються до всього інтерфейсу, повідомлень про помилки та
форматів дат.

## Завантажити

Готові установники (NSIS та MSI, Windows 10/11 x64) — на сторінці
[релізів](https://github.com/loxord241/Cobble-Launcher/releases/latest).
Після встановлення оновлення приходять самі; вручну — кнопка «Перевірити
оновлення» в налаштуваннях.

## Збірка з вихідного коду

Потрібні Node.js 20+, Rust (stable) і npm.

```bash
npm install        # залежності фронтенду
npm run tauri dev  # запуск у режимі розробки
```

Релізна збірка (установники NSIS та MSI у `src-tauri/target/release/bundle/`):

```bash
npm run tauri build
```

Перевірки якості:

```bash
npm run build                # TypeScript strict + Vite
cd src-tauri && cargo test   # 457 тести ядра + 10 інтеграційних
```

## Стек і структура

| Шар | Технології |
|---|---|
| Інтерфейс | React 19, TypeScript strict, Tailwind 4 |
| Ядро | Rust, Tauri 2, tokio |
| Контент | Modrinth API, маніфести Mojang, Adoptium |

- `src/` — інтерфейс: сторінки, компоненти, теми, i18n (ru, en, uk, pl)
- `src-tauri/src/` — ядро: інстанси та запуск, завантаження з перевіркою
  хешів, Modrinth, авторизація, автооновлення
- `docs/` — [карта документації](docs/README.md):
  [контракт ядро↔UI](docs/UI-CONTRACT.md),
  [реєстр рішень](docs/DECISIONS.md), [статус етапів](docs/PROGRESS.md)
- `scripts/` — приймальні тести: наскрізні перевірки UI, життєвого циклу
  процесу, інтеграції Modrinth

## Ліцензія і товарні знаки

Код проєкту — під ліцензією **MIT** (див. [LICENSE](LICENSE)); прямі
залежності та їхні ліцензії — у
[THIRD-PARTY-LICENSES.md](THIRD-PARTY-LICENSES.md),
атрибуція та товарні знаки — у [NOTICE](NOTICE).

**Не официальный продукт Minecraft. Не связан с Mojang и Microsoft.**

**Not an official Minecraft product. Not approved by or associated with Mojang or Microsoft.**

**Не офіційний продукт Minecraft. Не пов'язаний з Mojang та Microsoft.**

**Nieoficjalny produkt Minecraft. Niepowiązany z Mojang i Microsoft.**

Minecraft — товарний знак Mojang Synergies AB. Назви Mojang, Microsoft, Xbox
і Minecraft використовуються лише для опису сумісності; лаунчер не
розповсюджує файли гри, а завантажує їх з офіційних серверів Mojang з
перевіркою хешів.
