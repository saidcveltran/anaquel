[English](README.md) | Español

<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/banner-dark.svg">
  <img src="assets/banner-light.svg" alt="Anaquel: una marca de estante de libros con la palabra ANAQUEL" width="520">
</picture>

# Anaquel

**Markdown de entrada. Un sitio de documentación estático, estricto y rápido de salida.**

[![CI](https://github.com/saidcveltran/anaquel/actions/workflows/ci.yml/badge.svg)](https://github.com/saidcveltran/anaquel/actions/workflows/ci.yml)
<img src="https://img.shields.io/badge/Estado-temprano%200.1-1F7A63?style=flat" alt="Estado: temprano, versión 0.1">
[![Licencia](https://img.shields.io/badge/Licencia-Apache--2.0-0F1B2D?style=flat)](LICENSE)
[![Versión](https://img.shields.io/github/v/release/saidcveltran/anaquel?include_prereleases&label=Versi%C3%B3n&color=1F7A63&style=flat)](https://github.com/saidcveltran/anaquel/releases)
<img src="https://img.shields.io/badge/Windows-compila%20en%20CI-0F1B2D?style=flat&logo=windows11&logoColor=white" alt="Windows compila en CI">
<img src="https://img.shields.io/badge/Linux-compila%20en%20CI-1F7A63?style=flat&logo=linux&logoColor=white" alt="Linux compila en CI">

</div>

Anaquel es un motor de documentación escrito en Rust que convierte Markdown en un sitio estático para Cloudflare Workers, sin IA en el producto.

> **Estado: desarrollo temprano; 0.1.1 es la última versión publicada y la 0.1.0 fue la primera.** El motor compila y comprueba una biblioteca, el servidor de vista previa funciona y una biblioteca de ejemplo de 271 páginas compila sin diagnósticos. No hay manual. De la 0.1.1 se comprobaron con los archivos reales de la versión los dos archivos, sus sumas SHA-256 y sus atestaciones de compilación, y el archivo de Windows se descomprimió y ejecutó `version`, `new`, `check --strict` y `build`. El instalador de shell se comprobó en Linux, en el modo que no toca tu `PATH`, solo con la 0.1.0; el instalador de PowerShell y el modo por defecto del instalador de shell no se comprobaron. No se ha probado ningún despliegue en Cloudflare. Lee [Cuándo no usar Anaquel](#cuándo-no-usar-anaquel) antes de empezar.

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/screenshot-dark.png">
    <img src="assets/screenshot-light.png" alt="Una página de la biblioteca de ejemplo con el tema folio: navegación del tomo a la izquierda, la página en el centro y su índice a la derecha" width="100%">
  </picture>
</p>

<h3 align="center">Construido con</h3>

<p align="center">
  <img src="https://img.shields.io/badge/Rust-1.99%20%C2%B7%20edici%C3%B3n%202024-161D28?style=for-the-badge&logo=rust&logoColor=white" alt="Rust 1.99, edición 2024">
  <img src="https://img.shields.io/badge/Markdown-CommonMark%20%2B%20GFM-161D28?style=for-the-badge&logo=markdown&logoColor=white" alt="Markdown, CommonMark y GFM">
  <img src="https://img.shields.io/badge/Pagefind-b%C3%BAsqueda-161D28?style=for-the-badge" alt="Búsqueda con Pagefind">
  <img src="https://img.shields.io/badge/Cloudflare-destino%20Workers-161D28?style=for-the-badge&logo=cloudflare&logoColor=white" alt="Cloudflare Workers como destino de despliegue">
  <img src="https://img.shields.io/badge/GitHub%20Actions-Linux%20y%20Windows-161D28?style=for-the-badge&logo=githubactions&logoColor=white" alt="GitHub Actions en Linux y Windows">
</p>

| Biblioteca | Versión | Para qué |
| --- | --- | --- |
| [`pulldown-cmark`](https://crates.io/crates/pulldown-cmark) | 0.13.6 | Leer CommonMark y GFM. |
| [`minijinja`](https://crates.io/crates/minijinja) | 3.1.0 | Las plantillas del tema. |
| [`blake3`](https://crates.io/crates/blake3) | 1.8.7 | Las huellas de los nombres de los recursos. |
| [`axum`](https://crates.io/crates/axum) y [`tokio`](https://crates.io/crates/tokio) | 0.8.9 y 1.54.1 | El servidor de vista previa. |
| [`notify`](https://crates.io/crates/notify) | 8.2.2 | Vigilar el origen para la vista previa. |
| [`clap`](https://crates.io/crates/clap) | 4.6.7 | La línea de comandos. |

Cada dependencia está fijada a una versión exacta. [THIRD-PARTY.md](THIRD-PARTY.md) las lista todas con su licencia.

## In English

**Anaquel** is a documentation engine written in Rust that turns Markdown into a static site for Cloudflare Workers, with no AI in the product. It is early (version 0.1), has no release and no manual. The full English version is [README.md](README.md). The documentation of the project is in English; this file is its Spanish translation.

## Contenido

- [Qué hace](#qué-hace)
- [Para quién es](#para-quién-es)
- [Instalación](#instalación)
- [Ejemplo de cinco minutos](#ejemplo-de-cinco-minutos)
- [Comandos](#comandos)
- [Comparación](#comparación)
- [Cuándo no usar Anaquel](#cuándo-no-usar-anaquel)
- [Cómo encaja](#cómo-encaja)
- [Desarrollo y pruebas](#desarrollo-y-pruebas)
- [Preguntas frecuentes](#preguntas-frecuentes)
- [Cambios](#cambios)
- [Licencia](#licencia)

## Qué hace

Un sitio es una **Biblioteca** (Library) hecha de **Tomos** (Volumes, los libros). Cada tomo tiene secciones y páginas. Los **Estantes** (Shelves) agrupan tomos bajo una dirección común; están en la versión de desarrollo y todavía no en una versión publicada (mira `examples/multi-shelf`).

| Función | Etapa | Detalle |
| --- | --- | --- |
| Markdown CommonMark y GFM con front matter YAML | Disponible (0.1) | Tablas, listas de tareas, tachado, notas al pie |
| Enlaces `[[término]]` a una entrada del glosario del mismo tomo | Disponible (0.1) | Las entradas son los encabezados `###` de `terms.md` o `glossary.md` |
| El build se detiene ante enlaces rotos, anclas rotas, URL duplicadas y términos desconocidos | Disponible (0.1) | Los diagnósticos tienen códigos estables `AQ####`; `--strict` falla también ante advertencias |
| Una página que desaparece sin alias es un error | Disponible (0.1) | Los alias se vuelven páginas de redirección y un archivo `_redirects` de Cloudflare |
| Se rechaza el HTML en bruto; solo pasan enlaces `http`, `https` y `mailto` | Disponible (0.1) | Ver [SECURITY.md](SECURITY.md) |
| Content-Security-Policy estricta, sin scripts ni estilos en línea | Disponible (0.1) | Se escribe en `_headers`; el servidor de vista previa aplica las mismas reglas |
| Búsqueda de texto completo | Disponible (0.1) | Con el programa externo [Pagefind](https://pagefind.app); ver [la decisión](docs/decisions/0001-search.md) |
| Vista previa local que recompila y recarga | Disponible (0.1) | `anaquel serve`, solo en `127.0.0.1` |
| Tema `folio` con esquemas claro y oscuro y la fuente Literata incluida | Disponible (0.1) | Sin npm ni framework; el movimiento respeta `prefers-reduced-motion` |
| Directivas: aviso, detalles, pestañas, tarjetas, pasos, columnas, incrustado | Disponible (0.1) | Una página cada una en [`examples/directives`](examples/directives); el conjunto es cerrado |
| Resaltado de sintaxis del código con valla, en ambos esquemas | Disponible (0.1) | Solo clases, sin estilos en línea; Rust, Python, JavaScript, JSON, YAML, shell, HTML, CSS, SQL, C, C++, Go, Java y más; los demás quedan en texto plano (aún sin TOML, TypeScript ni PowerShell) |
| Fuzzing de los analizadores | Disponible (0.1) | [`fuzz/`](fuzz): objetivos de libFuzzer que corren 10 minutos en CI solo en Linux; en todos los sistemas una prueba de humo reproduce el corpus; ver [`fuzz/README.md`](fuzz/README.md) |
| Estantes, varios idiomas, `relocate`, `upgrade`, documentación versionada | Prevista (M2) | Nada de esta fila está en el código hoy |

## Para quién es

- **Autores de un manual** que quieren que el build falle ante un enlace roto en vez de publicarlo.
- **Quien despliega en Cloudflare Workers** y quiere las cabeceras y las redirecciones generadas con el sitio.
- **Quien contribuye** y prefiere un workspace de Rust pequeño, con lints estrictos y sin uso oculto de la red.

## Instalación

Descarga el archivo de tu sistema desde la [página de versiones](https://github.com/saidcveltran/anaquel/releases), compáralo con las sumas SHA-256 publicadas junto a él, descomprímelo y ejecuta `anaquel version`. O compila desde el código: necesitas Rust 1.99 o posterior ([rustup](https://rustup.rs)) y Git.

```text
git clone https://github.com/saidcveltran/anaquel.git
cd anaquel
cargo build --release -p anaquel
```

| Sistema | Ejecutar |
| --- | --- |
| Windows (PowerShell) | `.\target\release\anaquel.exe version` |
| Linux | `./target/release/anaquel version` |

La búsqueda necesita el programa `pagefind` junto a `anaquel` o en el `PATH` (o la variable `ANAQUEL_PAGEFIND` con su ruta). Sin él, el sitio se compila igual, sin cuadro de búsqueda. `anaquel doctor` te dice en qué caso estás.

Las versiones traen sumas SHA-256. No están firmadas por Windows, así que SmartScreen avisará; es lo esperado. Solo se compilan y prueban Windows x86_64 y Linux x86_64. macOS y Linux arm64 no están soportados: nadie ejecuta Anaquel ahí, y las versiones posteriores a la 0.1.0 no traen archivo para macOS (la 0.1.0 conserva dos que ya no se mantienen), así que el instalador de una versión posterior falla en un Mac porque no encuentra archivo para ese sistema; es lo esperado, no un error.

## Ejemplo de cinco minutos

```text
anaquel new my-docs --title "My docs"
anaquel check my-docs --strict
anaquel serve my-docs
anaquel build my-docs
```

`serve` imprime `http://127.0.0.1:1111/`; edita un archivo de `my-docs/volumes/guide/` y la página se recarga. `build` escribe el sitio en `my-docs/dist`, listo para subir a cualquier alojamiento estático. La integración continua ejecuta `new`, `check` y `build` en Linux y Windows en cada cambio, así que estos comandos están probados. Publicar en Cloudflare Workers aún no está documentado.

## Comandos

`anaquel --help` imprime el uso completo. El código de salida 1 significa que se hallaron problemas en el contenido; 2, que el propio comando falló.

| Comando | Qué hace |
| --- | --- |
| `new CARPETA [--title T]` | Crea una biblioteca pequeña; rechaza una carpeta que no esté vacía |
| `check [RAÍZ] [--strict]` | Lee la biblioteca e informa de los diagnósticos, no escribe nada |
| `build [RAÍZ] [--out DIR] [--no-search] [--strict]` | Escribe el sitio estático |
| `serve [RAÍZ] [--port N] [--search]` | Vista previa en `127.0.0.1` que recompila y recarga al cambiar |
| `doctor` | Informa de las herramientas que Anaquel encuentra en este equipo |
| `version` | Imprime la versión |

## Comparación

Hechos tal como los conozco; comprueba cada proyecto antes de decidir.

| | Anaquel | [Hugo](https://gohugo.io) | [mdBook](https://rust-lang.github.io/mdBook/) | [Starlight](https://starlight.astro.build) | [GitBook](https://www.gitbook.com) |
| --- | --- | --- | --- | --- | --- |
| Tipo | Programa que ejecutas | Programa que ejecutas | Programa que ejecutas | Framework que ejecutas | Servicio alojado |
| Escrito en | Rust | Go | Rust | JavaScript (Astro) | No aplica |
| Necesita Node y npm | No | No | No | Sí | No |
| Licencia | Apache-2.0 | Apache-2.0 | MPL-2.0 | MIT | Servicio propietario |
| El build se detiene ante un enlace roto | Sí | No por defecto | Necesita una herramienta extra | Necesita un complemento extra | No aplica |
| Madurez | 0.1, un mantenedor | Años de versiones | Años de versiones | Años de versiones | Producto comercial |

La última fila es la que cuenta hoy.

## Cuándo no usar Anaquel

- Necesitas algo que funcione hoy en producción: Anaquel está en la 0.1 y no tiene manual.
- Dependes de un ecosistema grande de temas, complementos e importadores: no existe, y una sola persona mantiene el proyecto.
- Necesitas documentación versionada, varios idiomas o una referencia OpenAPI: están previstas, no construidas.
- Quieres búsqueda sin instalar otro programa: Pagefind es externo.
- Necesitas una interfaz en un idioma distinto del inglés: los textos del tema están solo en inglés por ahora.

## Cómo encaja

El build es una cadena: ingest, catalog, compose, render, emit, index. Cada etapa es un crate.

| Crate | Qué contiene |
| --- | --- |
| `anaquel` | La línea de comandos |
| `anaquel-schema` | Los esquemas de configuración y de front matter |
| `anaquel-diagnostics` | Los códigos `AQ####` y sus mensajes |
| `anaquel-directive` | La sintaxis de bloques `:::` |
| `anaquel-catalog` | El árbol de tomos y páginas, URL, enlaces, términos y el registro de URL perdidas |
| `anaquel-compose` | De Markdown a HTML seguro |
| `anaquel-theme`, `anaquel-render` | El tema `folio` y las plantillas de página |
| `anaquel-assets`, `anaquel-emit` | Recursos con huella, `_headers`, `_redirects`, `sitemap.xml`, 404 |
| `anaquel-search` | El puerto hacia Pagefind |
| `anaquel-pipeline` | Las etapas en orden |
| `anaquel-preview` | El servidor de vista previa |

## Desarrollo y pruebas

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p anaquel -- check examples/reference-library --strict
python -I tools/check_images.py
```

El código `unsafe` está prohibido, la documentación que falta es un error y clippy corre con `pedantic`. `examples/reference-library` es una biblioteca inventada de 271 páginas que sirve para probar el motor. Ver [CONTRIBUTING.md](CONTRIBUTING.md).

## Preguntas frecuentes

**¿Está listo para usar?** No. Es la versión 0.1, la primera publicada. Compila y sus pruebas pasan en CI en tres sistemas.

**¿Usa IA?** No. No hay IA en el producto, ni en el build, ni en el sitio que escribe.

**¿Llama a algún servidor?** No. El servidor de vista previa escucha solo en `127.0.0.1` y el sitio compilado no carga nada de fuera de su propio host.

**¿Dónde está el manual?** Aún no está escrito. Este README, `anaquel --help` y la biblioteca de ejemplo son todo lo que hay.

## Cambios

Ver [CHANGELOG.md](CHANGELOG.md). Cada cambio queda anotado con sus límites conocidos.

## Licencia

Apache License 2.0: ver [LICENSE](LICENSE) y [NOTICE](NOTICE). Software de terceros: [THIRD-PARTY.md](THIRD-PARTY.md). El nombre y el logotipo no están cubiertos por la licencia: ver [TRADEMARKS.md](TRADEMARKS.md).

Informes de seguridad: [SECURITY.md](SECURITY.md). Cómo participar: [CONTRIBUTING.md](CONTRIBUTING.md) y el [Código de conducta](CODE_OF_CONDUCT.md).
