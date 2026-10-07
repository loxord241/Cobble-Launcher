<div align="center">

<sub>[English](README.md) · [Русский](README.ru.md) · [Українська](README.uk.md) · <b>Polski</b></sub>

<img src="src-tauri/icons/128x128.png" width="88" alt="Cobble Launcher">

# Cobble Launcher

**Uczciwy launcher Minecraft dla Windows**

[![Wersja](https://img.shields.io/github/v/release/loxord241/Cobble-Launcher?style=flat-square&label=wersja)](https://github.com/loxord241/Cobble-Launcher/releases/latest)
[![Licencja](https://img.shields.io/github/license/loxord241/Cobble-Launcher?style=flat-square)](LICENSE)
[![Platforma](https://img.shields.io/badge/Windows-10%2F11_x64-blue?style=flat-square&logo=windows)](https://github.com/loxord241/Cobble-Launcher/releases/latest)
[![Języki](https://img.shields.io/badge/języki-ru·en·uk·pl-green?style=flat-square)](#języki)
[![Testy](https://img.shields.io/badge/testy-457%2B10-success?style=flat-square)](#budowanie-z-kodu-źródłowego)

**[Pobierz najnowszą wersję](https://github.com/loxord241/Cobble-Launcher/releases/latest)** ·
[Zgłoś problem](https://github.com/loxord241/Cobble-Launcher/issues) ·
[Wideo demo](docs/anim-demo.mp4)
· [Strona Cobble Launcher](https://loxord241.github.io/Cobble-Launcher/)

<img src="docs/screenshots/home.png" alt="Katalog Modrinth na stronie głównej" width="820">

Tauri 2 · Rust · React 19 · TypeScript

</div>

---

Cobble — desktopowy launcher z natywnym rdzeniem w Ruście i lekkim interfejsem w
Reakcie. Żadnych silników przeglądarkowych w tle, żadnych „przyspieszaczy” ani
iniekcji do gry: wyłącznie oficjalne źródła, weryfikacja każdego pliku po hashu
i zestaw sodium do optymalizacji.

## Katalog Modrinth

- Katalog na stronie głównej: paczki modów i mody, wyszukiwanie, sortowanie
  (pobrania / ulubione / aktualizacje), filtr loadera, nieskończone przewijanie
- Okno projektu jak w CurseForge: Przegląd / Dziennik zmian / Galeria / Wersje
- Instalacja paczek modów do nowej instancji i modów — prosto z okna projektu
  lub ze strony instancji; zbiorcza aktualizacja modów jednym kliknięciem

## Instancje

- Izolowane instancje: własna wersja, loader (Fabric / Quilt / Forge /
  NeoForge), mody i ustawienia
- Postęp pobierania i status na żywo wprost na karcie; ochrona przed
  podwójnym uruchomieniem i uszkodzeniem działającej instancji — na poziomie
  rdzenia
- Pełna kontrola: RAM, wersja Javy (automatyczna instalacja Adoptium), presety
  flag JVM, szybki start (Quick Play), profile dla instancji, zmiana nazwy,
  duplikowanie, kopie zapasowe i snapshoty modów z możliwością przywrócenia
- Światy, zrzuty ekranu i configi modów — wprost ze strony instancji

## Profile

- Profile Microsoft (OAuth w przeglądarce), Ely.by (authlib-injector) i
  offline; profile offline — uczciwie oznaczone jako nieoficjalne
- Awatary: głowa skina dla Microsoft / Ely.by, klasyczny Steve — dla trybu
  offline
- Refresh-tokeny — wyłącznie w systemowym keyringu Windows
- Discord Rich Presence: nazwa instancji w statusie Discorda

<div align="center">
  <table>
    <tr>
      <td><img src="docs/screenshots/instances.png" alt="Instancje: karty z grafikami i wersjami" width="420"></td>
      <td><img src="docs/screenshots/accounts.png" alt="Zarządzanie profilami: prawdziwy Steve dla profili offline" width="420"></td>
    </tr>
    <tr>
      <td><img src="docs/screenshots/instance.png" alt="Strona instancji: uruchamianie, sekcje, statystyki" width="420"></td>
      <td><img src="docs/screenshots/settings.png" alt="Ustawienia: motywy, język, pamięć" width="420"></td>
    </tr>
  </table>
</div>

## Automatyczne aktualizacje

Launcher sam sprawdza wydania (raz na sesję, po cichu) i pokazuje plakietkę
„Dostępna aktualizacja”. Instalacja jednym przyciskiem: podpisana aktualizacja
(minisign), postęp pobierania, automatyczny restart. Twoje instancje, profile
i ustawienia zostają na miejscu.

## Bezpieczeństwo i prywatność

- **Tylko oficjalne źródła**: Mojang, Modrinth, Adoptium — każdy plik jest
  sprawdzany z hashem pobranym z serwera. Mirrory bez weryfikacji hash są
  sprzeczne z zasadami projektu.
- **Zero iniekcji do gry**: launcher nie modyfikuje procesu gry Minecraft.
  Jedyny wyjątek — authlib-injector dla Ely.by, działający w ramach tej usługi.
- **Sekrety nie opuszczają systemu**: tokeny są przechowywane w keyringu
  Windows, a w logach włączone jest automatyczne ukrywanie sekretów.

## Języki

Русский · English · Українська · Polski — przełączane w onboardingu i
ustawieniach, obejmują cały interfejs, komunikaty o błędach i formaty dat.

## Pobieranie

Gotowe instalatory (NSIS i MSI, Windows 10/11 x64) — na stronie
[wydań](https://github.com/loxord241/Cobble-Launcher/releases/latest).
Po instalacji aktualizacje przychodzą same; ręcznie — przycisk „Sprawdź
aktualizacje” w ustawieniach.

## Budowanie z kodu źródłowego

Wymagane: Node.js 20+, Rust (stable) i npm.

```bash
npm install        # zależności frontendu
npm run tauri dev  # uruchomienie w trybie deweloperskim
```

Kompilacja release (instalatory NSIS i MSI w `src-tauri/target/release/bundle/`):

```bash
npm run tauri build
```

Kontrole jakości:

```bash
npm run build                # TypeScript strict + Vite
cd src-tauri && cargo test   # 457 testy rdzenia + 10 integracyjnych
```

## Stos technologiczny i struktura

| Warstwa | Technologie |
|---|---|
| Interfejs | React 19, TypeScript strict, Tailwind 4 |
| Rdzeń | Rust, Tauri 2, tokio |
| Zawartość | Modrinth API, manifesty Mojang, Adoptium |

- `src/` — interfejs: strony, komponenty, motywy, i18n (ru, en, uk, pl)
- `src-tauri/src/` — rdzeń: instancje i uruchamianie, pobieranie z weryfikacją
  hash, Modrinth, autoryzacja, automatyczne aktualizacje
- `docs/` — [mapa dokumentacji](docs/README.md):
  [kontrakt rdzeń↔UI](docs/UI-CONTRACT.md),
  [rejestr decyzji](docs/DECISIONS.md), [status etapów](docs/PROGRESS.md)
- `scripts/` — testy akceptacyjne: kompleksowe sprawdzenia UI, cyklu życia
  procesu i integracji z Modrinth

## Licencja i znaki towarowe

Kod projektu — na licencji **MIT** (patrz [LICENSE](LICENSE)); bezpośrednie
zależności i ich licencje — w [THIRD-PARTY-LICENSES.md](THIRD-PARTY-LICENSES.md),
atrybucje i znaki towarowe — w [NOTICE](NOTICE).

**Не официальный продукт Minecraft. Не связан с Mojang и Microsoft.**

**Not an official Minecraft product. Not approved by or associated with Mojang or Microsoft.**

**Не офіційний продукт Minecraft. Не пов'язаний з Mojang та Microsoft.**

**Nieoficjalny produkt Minecraft. Niepowiązany z Mojang i Microsoft.**

Minecraft jest znakiem towarowym Mojang Synergies AB. Nazwy Mojang, Microsoft,
Xbox i Minecraft są używane wyłącznie do opisania kompatybilności; launcher
nie rozpowszechnia plików gry, lecz pobiera je z oficjalnych serwerów Mojang
z weryfikacją hash.
