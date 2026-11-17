# Anaquel

Anaquel construye un sitio estático de documentación a partir de una carpeta de
ficheros Markdown. Un sitio es una **Biblioteca** (*Library*) formada por
**Volúmenes** (*Volume*); cada volumen tiene secciones y páginas. El resultado
son ficheros planos que puedes alojar donde quieras, con `_headers` y
`_redirects` listos para Cloudflare.

Estado: desarrollo temprano (0.1, sin publicar). Versión en inglés: [README.md](README.md).

## Qué hace

- Markdown CommonMark y GFM con front matter YAML.
- `[[término]]` enlaza con una entrada del glosario del mismo volumen.
- Enlaces rotos, anclas rotas, URL duplicadas y términos desconocidos son errores
  que detienen la construcción: un sitio publicado no los tiene.
- Una página que desaparece sin alias es un error: no hay URL muertas en silencio.
- El HTML en bruto en el contenido se rechaza; solo pasan enlaces `http`, `https`
  y `mailto`.
- El sitio lleva una Content-Security-Policy estricta, sin scripts ni estilos en línea.
- Búsqueda de texto completo con [Pagefind](https://pagefind.app) (opcional).
- Servidor de vista previa en `127.0.0.1` que reconstruye y recarga al cambiar.

## Inicio rápido

```text
anaquel new mis-docs --title "Mis docs"
anaquel serve mis-docs
anaquel build mis-docs
```

Órdenes: `new`, `build`, `check`, `serve`, `doctor`, `version`.
Con `--strict` los avisos cuentan como errores.

## Estructura de una biblioteca

```text
anaquel.toml              schema, engine, title, base_url, language
volumes/<slug>/volume.toml
volumes/<slug>/_index.md  portada del volumen (obligatoria)
volumes/<slug>/**/*.md    páginas; el `_index.md` de una carpeta es su página de sección
volumes/<slug>/terms.md   glosario: cada encabezado `###` es un término
```

Solo las páginas llamadas `terms.md` o `glossary.md` definen términos. Los
enlaces se escriben hacia el fichero fuente (`[instalar](setup.md)`) y el motor
los resuelve.

## Compilar desde el código

Rust 1.99 o superior.

```text
cargo build --release
cargo test --workspace
cargo run -p anaquel -- check examples/reference-library --strict
```

`examples/reference-library` es una biblioteca inventada de más de 270 páginas
que sirve para probar el motor.

## Límites conocidos

Véase [CHANGELOG.md](CHANGELOG.md).

## Licencia

Apache-2.0. Véase [LICENSE](LICENSE).
