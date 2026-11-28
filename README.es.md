# Suppa Browser

<a href="https://www.buymeacoffee.com/Un1que0ff"><img src="https://cdn.buymeacoffee.com/buttons/v2/default-violet.png" alt="Buy me a coffee" height="50"></a>

[English](README.md) · [Русский](README.ru.md) · [Deutsch](README.de.md) · **Español** · [中文](README.zh.md)

**Un navegador pequeño, rápido y privado para Windows (y Linux): un solo `.exe` de unos 3 MB con bloqueador de anuncios, extensiones de Chrome, grupos de pestañas y una interfaz totalmente personalizable.**

Escrito en Rust sobre Microsoft **WebView2**, el motor Chromium que ya forma parte de Windows 10/11: las páginas se ven igual que en Chrome/Edge, mientras Suppa sigue siendo muy pequeño.

![Suppa Browser](docs/screenshots/main-dark.png)

## Características

- **Diminuto y rápido**: la ventana está lista en unos 0,2 s; las pestañas en segundo plano se suspenden y descargan para ahorrar CPU y memoria (puedes mantener activas las que elijas).
- **Privado**: sin cuenta, sin telemetría, sin analítica.
- **Bloqueador de anuncios y rastreadores** (motor de Brave) y **anuncios de YouTube saltados**: Suppa pulsa «Omitir» por ti.
- **Avisos de cookies**: por defecto solo las necesarias («Rechazar todo»), o «Aceptar todo» si lo prefieres.
- **Direcciones, tarjetas y PayPal**: varias direcciones y tarjetas rellenadas con un clic; las tarjetas tras Windows Hello y el CVC nunca se guarda.
- **Contraseñas seguras**: los formularios de registro reciben una contraseña como `A%$3G&8^ga12Kq` que Suppa rellena y guarda.
- **Sincronizar y compartir con Dropbox** (opcional): contraseñas, direcciones y tarjetas en un archivo cifrado con tu frase de contraseña.
- **Reproductor y volumen por pestaña**: portada de Spotify / Tidal, vista previa de YouTube, pausa, siguiente y un volumen propio para cada pestaña, independiente del de Windows.
- **Guardar vídeo / MP3**: con clic derecho en un vídeo o en una página de YouTube.
- **Extensiones de Chrome**, grupos de pestañas, pestañas guardadas, historial, descargas, gestor de contraseñas, copias de seguridad.
- **Tu aspecto**: tema como Windows (por defecto), color de acento de Windows, tintes, colores propios, barras arriba, abajo o a los lados.
- **Idiomas**: English, Русский, Deutsch, Español, 中文 (por defecto el idioma de Windows; Ajustes → Apariencia → Idioma).
- **Atajos de teclado** como en Chrome, Edge y Firefox: F1 muestra la lista.

## Instalación

**Requisitos:** Windows 10 u 11 (64 bits) con Microsoft Edge WebView2 Runtime (incluido en Windows actualizado).

**Instalador:** descarga **`SuppaSetup.exe`** de la página [Releases](../../releases) y ejecútalo. No necesita permisos de administrador; Suppa se actualiza solo.

**Compilarlo tú mismo:** haz doble clic en **`build.bat`**. Instala lo que falte (Rust, Microsoft C++ Build Tools, WebView2) y genera `dist\Suppa.exe` y `dist\SuppaSetup.exe`.

---

Todas las funciones, la privacidad y la estructura del código, en detalle en el [README en inglés](README.md).
