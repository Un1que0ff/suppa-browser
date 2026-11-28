# Suppa Browser

[English](README.md) · [Русский](README.ru.md) · **Deutsch** · [Español](README.es.md) · [中文](README.zh.md)

**Ein kleiner, schneller, privater Browser für Windows (und Linux): eine einzige `.exe` mit etwa 3 MB, mit eingebautem Werbeblocker, Chrome-Erweiterungen, Tab-Gruppen und einer frei anpassbaren Oberfläche.**

Geschrieben in Rust auf Basis von Microsoft **WebView2**, der Chromium-Engine, die schon in Windows 10/11 steckt – Seiten sehen also genau aus wie in Chrome/Edge, während Suppa selbst winzig bleibt.

![Suppa Browser](docs/screenshots/main-dark.png)

## Funktionen

- **Winzig und schnell** – das Fenster ist nach etwa 0,2 s bereit; Hintergrund-Tabs schlafen und werden entladen, um CPU und RAM zu sparen (ausgewählte Tabs kannst du wach halten).
- **Privat** – kein Konto, keine Telemetrie, keine Analyse.
- **Werbe- und Trackerblocker** (Brave-Engine) und **YouTube-Werbung übersprungen** – Suppa drückt „Überspringen“ für dich.
- **Cookie-Hinweise** – standardmäßig nur notwendige Cookies („Alle ablehnen“), auf Wunsch „Alle akzeptieren“.
- **Adressen, Karten und PayPal** – mehrere Adressen und Karten, mit einem Klick ausgefüllt; Karten erst nach Windows Hello, die CVC wird nie gespeichert.
- **Sichere Passwörter** – Registrierungsformulare bekommen ein Passwort wie `A%$3G&8^ga12Kq`, das Suppa ausfüllt und speichert.
- **Synchronisieren und Teilen über Dropbox** (optional) – Passwörter, Adressen und Karten in einer Datei, verschlüsselt mit deiner Passphrase.
- **Player und Lautstärke pro Tab** – Cover von Spotify / Tidal, Videovorschau von YouTube, Pause, Weiter und eine eigene Lautstärke pro Tab, unabhängig von der Windows-Lautstärke.
- **Video / MP3 speichern** – per Rechtsklick auf ein Video oder eine YouTube-Seite.
- **Chrome-Erweiterungen**, Tab-Gruppen, gespeicherte Tabs, Verlauf, Downloads, Passwortmanager, Sicherungen.
- **Dein Aussehen** – Design wie Windows (Standard), Windows-Akzentfarbe, Tönung, eigene Farben, Leisten oben, unten oder seitlich.
- **Sprachen** – English, Русский, Deutsch, Español, 中文 (standardmäßig die Windows-Sprache; Einstellungen → Darstellung → Sprache).
- **Tastenkürzel** wie in Chrome, Edge und Firefox – F1 zeigt die Liste.

## Installation

**Voraussetzungen:** Windows 10 oder 11 (64 Bit) mit der Microsoft Edge WebView2 Runtime (in aktuellen Windows-Versionen enthalten).

**Setup:** lade **`SuppaSetup.exe`** von der Seite [Releases](../../releases) herunter und starte es. Keine Administratorrechte nötig; Suppa aktualisiert sich selbst.

**Selbst bauen:** doppelklicke **`build.bat`**. Es installiert alles Fehlende (Rust, Microsoft C++ Build Tools, WebView2) und baut `dist\Suppa.exe` und `dist\SuppaSetup.exe`.

---

Alle Funktionen, Datenschutz und Aufbau des Codes ausführlich im [englischen README](README.md).
