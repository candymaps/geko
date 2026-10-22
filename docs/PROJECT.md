# geko — el host

## 1. Objetivo

geko es un motor geoespacial modular y multipaís. Este repositorio es **el host**: el binario `geko`, que lee la configuración, resuelve el ámbito, baja y carga los módulos, publica y recarga los bundles y sirve lo común por HTTP. Cada servicio es un **módulo** con su propio repositorio, que el host conoce solo por el contrato de `geko-module-sdk` (docs/MODULES_PLAN.md):

| Módulo | Repositorio | Estado |
|---|---|---|
| `geocoder` | [candymaps/geko-geocoder](https://github.com/candymaps/geko-geocoder) | Búsqueda, autocompletado e inversa, con los packs de país; su especificación, en su `docs/PROJECT.md` |
| `tiles` | [candymaps/geko-tiles](https://github.com/candymaps/geko-tiles) | Esqueleto: responde 503 |

Todo el ciclo es **automático**: se declaran en `geko.toml` el ámbito y los módulos, y geko baja los módulos, les pide que descarguen y generen lo suyo para el ámbito, publica el bundle y lo sirve (sección 7).

Los objetivos medibles de cada servicio (latencia, caudal, acierto frente a Pelias) son de su módulo. Los del host: que el salto hasta el módulo no se note en sus medidas (medido con el geocoder y España, docs/MODULES_PLAN.md, paso 2) y que un fallo de un módulo no tire el servicio (`ProcessModule`).

### 1.1 No objetivos (por ahora)

- Enrutamiento.
- Actualización por minuto: se regenera por lotes.

## 2. Principios

### 2.1 Abstracción selectiva

Se abstrae **solo donde hay una alternativa realista o la decisión técnica está abierta**. Un trait diseñado antes de tener dos implementaciones tiende a copiar la forma de la primera. Por eso:

1. Los puertos de la sección 4 son los únicos traits del host. Lo demás (transporte HTTP, almacenamiento de bundles, registro de módulos) es código normal: **su trait se extrae cuando aparezca una segunda implementación real**.
2. Los traits solo exponen tipos de la SDK. Ninguna tecnología concreta (`ureq`, `axum`, `libloading`…) sale de su crate.
3. Cada puerto tiene una **suite de conformidad** (sección 12) que toda implementación debe superar.
4. Cada módulo de un bundle lleva en el manifiesto su versión y su contrato; el host no carga un bundle de otro contrato.

### 2.2 Módulos

- **Un módulo desactivado no consume recursos:** no se descarga, no se instancia, no abre ficheros y no registra rutas.
- **Activación** con `[modules]` en `geko.toml` o `--modules` en la línea de órdenes. Al menos uno activo.
- **Origen**, por orden: enlazado en el binario (ninguno en el de este repositorio; un módulo puede enlazarse con `geko_app::main` para sus propias pruebas), una carpeta de `GEKO_MODULES` (con su `module.toml`, como proceso o biblioteca: `kind = "process" | "dylib"`) o descargado y firmado de `ghcr.io/candymaps/geko-<módulo>` (sección 7). Con `GEKO_MODULES`, un módulo que no está ahí ni enlazado es un error, nunca una descarga.
- **Independencia:** el host no depende de ningún módulo, y ningún módulo de otro. Lo que comparten va a la SDK.
- **Rutas** bajo el nombre del módulo (`/geocoder/…`, `/tiles/…`): dos módulos no pueden pisarse.
- **Configuración** propia de cada módulo, `geko-<módulo>.toml`, junto a `geko.toml`: el host no la lee, se la pasa.
- **Bundles parciales:** el manifiesto declara qué módulos contiene. `geko serve` usa la versión de cada módulo que generó el bundle que sirve, y la baja si falta.

### 2.3 Varios países

- **Códigos ISO:** el ámbito se declara con ISO 3166-1 (país) e ISO 3166-2 (región). El host solo valida los códigos y los pasa a los módulos; qué significa una región para cada uno (las particiones del geocoder, sus packs de país) es del módulo.
- **Paquetes de datos de un módulo** (los packs de país del geocoder): el módulo los baja con el mecanismo de artefactos firmados de la SDK (`geko-artifact`), el mismo con que el host baja los módulos. El host no sabe qué es un pack.

## 3. Arquitectura

La generación y el servicio pueden correr en **máquinas distintas**: la generación necesita mucha RAM y CPU durante minutos u horas; el servicio, poca.

```
 ┌───────────────────────────── nodo de generación ──────────────────────────────┐
 │  geko.toml ─► módulos activos ─► abrir cada uno (enlazado, GEKO_MODULES o      │
 │               descarga firmada) ─► plan / fetch / build de cada módulo         │
 │                                     (Downloader del host, su parte del bundle) │
 │                                       ▼                                        │
 │  bundle/vN/ (las partes de los módulos + manifest.json) ─► publicación         │
 └────────────────────────────────────────────────────────────────────────────────┘
                                │ descarga solo de lo que cambió
 ┌────────────────────────────── nodo de servicio ───────────────────────────────┐
 │  recarga: descarga, valida, carga cada módulo, cambio atómico (ArcSwap)        │
 │  HTTP (axum): /health, /metrics, /v1/attribution, /docs, claves, CORS, límites │
 │            └─► /<módulo>/… ─► Module::handle (enlazado, socket o biblioteca)   │
 └────────────────────────────────────────────────────────────────────────────────┘
```

### 3.1 Comandos

| Comando | Hace |
|---|---|
| `geko plan` | Muestra los módulos (versión, origen), y qué descargaría y generaría cada uno, con sus licencias, sin hacer nada |
| `geko fetch` | Solo descarga lo que piden los módulos |
| `geko build` | Descarga lo que falte o haya cambiado, genera la parte de cada módulo y publica el bundle |
| `geko serve` | Sirve un bundle. **Nunca descarga fuentes**: solo bundles ya generados y, si falta, el módulo que lo generó |
| `geko up` | `serve` y, en segundo plano, `build`: ahora y cada `[data] max_age_days`, sin cron. Sirve el bundle publicado mientras genera el nuevo y lo recoge al terminar; si un build falla, sigue con el que tiene y reintenta a la hora. Con `offline` o `refresh = never` genera una sola vez. Es la orden de la imagen de Docker |
| `geko purge [data | packages [--all] | modules]` | Borra lo que la configuración ya no pide: descargas y ficheros de trabajo de los módulos, sus paquetes de datos fuera del ámbito y los módulos que ningún módulo activo ni bundle conservado usa; sin subcomando, todo. `build --purge` y `up --purge` lo hacen antes de generar |
| `geko <módulo> <orden>` | Las órdenes de un módulo activo; `geko <módulo>` las lista |

### 3.2 Plataformas

El servicio de producción es Linux. El desarrollo puede hacerse en Windows o WSL; `ProcessModule` (socket Unix) solo existe en Unix.

## 4. Puertos (traits)

| Puerto | Responsabilidad | Implementaciones |
|---|---|---|
| `Module` | El contrato de un módulo: describirse, planificar, descargar, generar su parte del bundle, cargarla, atender peticiones, métricas, órdenes | `StaticModule` (enlazado), `ProcessModule` (proceso propio, socket Unix), `DylibModule` (biblioteca cargada en el proceso) |
| `Downloader` | Descargar ficheros con caché, reanudación, ETag, reintentos y sumas de control | `geko-fetch` (HTTP); descargadores falsos en las pruebas |

El contrato está en `crates/geko-module-sdk` (`module.rs`), con su número (`CONTRACT`). Un cambio que lo rompe sube el número; uno aditivo, no. Cada módulo publicado lleva el contrato que habla, y el host baja el último de su contrato.

## 5. Configuración

La configuración está siempre en `/etc/geko/geko.toml`, con la de cada módulo a su lado, y los datos siempre en `/var/lib/geko`; ninguna de las dos rutas se cambia desde el fichero ni desde la línea de órdenes. `GEKO_CONFIG` y `GEKO_DATA` las cambian para las comprobaciones. Todas las opciones, con sus valores por defecto, están en el README, «Configuration reference».

```toml
[modules]
geocoder = true                 # o una tabla: { enabled, config, version, registry }
tiles    = false

[[scope]]
country = "ES"
regions = ["ES-PM"]             # ISO 3166-2; vacío = país entero

[data]
refresh         = "if-changed"  # never | if-changed | always
max_age_days    = 30
keep_downloads  = false
offline         = false
accept_licenses = false         # debe ser true para descargar (sección 7.4)
parallel_downloads = 4

[build]
keep = 1                        # versiones publicadas que se conservan

[serve]
listen       = "0.0.0.0:8080"
reload_secs  = 30
cors_origins = []
api_keys     = []
rate_limit_per_s = 0
rate_limit_burst = 0
bundles_url  = ""
attribution_contact = ""
```

## 6. Licencias y atribución

**Decisión (2026-10-21):** el código se licencia con AGPL-3.0-only ([LICENSE](../LICENSE)), y toda contribución externa exige firmar antes el CLA ([CLA.md](../CLA.md)), para poder cambiar de licencia o dar licencias comerciales.

- Cada módulo declara las licencias de lo que descarga; `geko plan` las lista, y pasan al manifiesto del bundle.
- `/v1/attribution` (sin clave de API): las licencias y atribuciones de las fuentes del bundle, a quién pedir el código de la base derivada (`[serve] attribution_contact`, ODbL 4.6) y lo que añade cada módulo (el geocoder, el aviso de base derivada y dónde está su método).

No es asesoría legal: antes de abrir el servicio al público, conviene que lo revise un abogado.

## 7. Descarga automática

### 7.1 Qué se descarga

**módulos activos → lo que cada módulo pide para el ámbito**. Un módulo desactivado no descarga nada. Los módulos que no están enlazados ni en `GEKO_MODULES` se bajan antes, de `ghcr.io/candymaps/geko-<módulo>` (`[modules.<módulo>] registry`), a `/var/lib/geko/modules/<módulo>/<versión>/`: el último de su contrato o el de `version`, solo si lleva una firma de `TRUSTED_KEYS` (`geko-artifact`) y es de la plataforma del host.

### 7.2 Mecánica (`geko-fetch`)

- Escritura en `.part` y renombrado al terminar: una descarga a medias nunca parece completa.
- Reanudación con HTTP Range.
- Reintentos con espera creciente y límite de conexiones (`parallel_downloads` en total).
- Descarga condicional con ETag e `If-Modified-Since`, según `refresh`.
- Verificación de sumas de control cuando la fuente las publica.
- Con `offline = true`, ninguna petición; si falta algo, error con la lista de lo que falta.
- Artefactos OCI (módulos, paquetes de datos) de registros como ghcr.io (`oci.rs`).

### 7.3 Caché

`raw/<fuente>/`, con un fichero `<fichero>.raw.json` junto a cada descarga: URL, ETag, `Last-Modified`, sha256, tamaño y fecha. Las descargas en curso usan `<fichero>.part` y `<fichero>.part.meta.json` para poder reanudarse.

La caché, la política de refresco y el modo sin red son del `Downloader`: el módulo solo dice qué URL necesita y dónde dejarla.

**Cuándo se comprueba** (`[data] max_age_days`, 30 por defecto): todas las fuentes a la vez, con una marca local de la última comprobación (`raw/.checked`). Mientras no vence, no se comprueba nada: se conserva cada descarga y solo se baja lo que falta. Al vencer, o con `geko build --refresh`, cada fuente pregunta si cambió y solo se baja lo que cambió. `refresh = always` lo baja todo y `never` no comprueba nunca.

**Descargas borradas** (`[data] keep_downloads = false`, por defecto): tras publicar un bundle, cada módulo borra lo que declara borrable y conserva sus fichas; una ficha sin su fichero cuenta como descarga conservada (`Downloader::keeps_removed`). Con `offline = true` no se borra nada.

### 7.4 Licencias

Descargar automáticamente implica aceptar las condiciones de cada fuente. Por eso `accept_licenses = true` es obligatorio para descargar, y `geko plan` lista antes todas las licencias.

## 8. Incrementalidad

Cada etapa de un módulo se identifica por el hash de sus entradas y de la versión de su código (`geko_module_sdk::incremental`). Si no cambia, la etapa no se repite.

- **Sello:** cada etapa escribe `<dir>.stamp.json` junto a su salida, con `key` (código y entradas) y `output` (contenido de la salida, sin informes con tiempos). Se borra antes de empezar y se escribe solo si termina bien: una etapa interrumpida nunca se toma por actual.
- **Determinismo:** como la generación es determinista, una etapa que se repite y escribe los mismos bytes no obliga a repetir las siguientes.
- **Publicación:** no se publica una versión nueva si la actual tiene las mismas partes, módulos y licencias.
- **Forzar:** `geko build --force` repite todas las etapas.

## 9. Parte caliente y parte fría

Los ficheros de un bundle bajo un directorio `hot/` se fijan en RAM al cargarlos; el resto se mapea sin fijar, y el sistema operativo guarda en caché lo consultado. Cada módulo decide qué pone en cada parte.

## 10. API

Las rutas del host están en [openapi.yaml](../openapi.yaml), servida en `/openapi.yaml`; las de cada módulo, en el suyo (`openapi` de su `describe`). `/docs` (Swagger UI) las muestra todas.

```
GET  /                  página de demostración (lo que ofrecen los módulos activos)
GET  /v1/attribution
GET  /openapi.yaml, /docs
GET  /health            módulos, versiones y los que no están disponibles (modules_unavailable)
GET  /metrics           las del host y las de cada módulo, bajo geko_
     /<módulo>/…        las de cada módulo activo; 404 si no lo está
```

Las claves de API se piden en todas las rutas salvo `/`, `/health`, `/v1/attribution` y las que un módulo declara públicas (`public`, como su demo).

## 11. Bundles y despliegue

```
bundles/
  v2026-10-15T030000/
    manifest.json             # versión, módulos (versión, contrato), licencias, hash de cada artefacto
    <lo de cada módulo>       # p. ej. segments/<país>/ del geocoder
  current -> v2026-10-15T030000
  current.version             # la versión de current, para quien la lee por HTTP
```

- **Generación**: `geko build` publica una versión nueva, enlazando (enlaces duros) lo que no cambió de la anterior. Nunca modifica una existente.
- **Servicio**: con `[serve] bundles_url` (la carpeta de bundles de otra máquina, servida por cualquier servidor web o almacenamiento de objetos), cada recarga lee `current.version` y baja **solo los artefactos cuyo hash cambió**. Las descargas pasan por `.downloads/` y la versión nueva solo pasa a `current` completa. Comprueba sumas de control, carga cada módulo y hace el cambio atómico con `ArcSwap`; las consultas en curso terminan sobre la versión anterior. Se conservan las últimas `[build] keep` versiones.
- **Escala**: primero réplicas detrás de un balanceador; el reparto entre nodos, si hace falta, es de cada módulo (el geocoder: `[serve] countries`).

## 12. Suites de conformidad

En `geko-module-sdk`, con la *feature* `conformance`:

- **`Module`** (`conformance::module::check`): sobre un módulo abierto con el ámbito vacío y sin bundle, `describe` lleva el contrato de la SDK, una versión y rutas válidas sin repetir; su `openapi` es una de sus rutas GET; órdenes sin repetir y error para una desconocida; `/metrics` en formato de Prometheus bajo `geko_`; cada ruta, con peticiones raras, responde sin 500 y con errores `{"error": …}` en JSON, también tras un bundle que no pudo cargar y desde 8 hilos a la vez. La pasa cada módulo desde sus pruebas y, en `geko-app`, un módulo de prueba por cada adaptador (enlazado, proceso y biblioteca).
- **`Downloader`**: la pasa `geko-fetch`, contra un servidor HTTP falso.

## 13. Estructura del repositorio

```
geko/
  Cargo.toml                 # workspace
  openapi.yaml               # rutas del host
  Dockerfile, develop/       # imagen ghcr.io/candymaps/geko, sin módulos; make start-dev
  ci/                        # test.sh (comprobaciones), affected.sh (cuáles toca un cambio)
  crates/
    geko-module-sdk/         # el contrato (Module), tipos compartidos, bundle, etapas, conformidad
    geko-fetch/              # Downloader HTTP, caché raw, cliente de registros OCI
    geko-artifact/           # artefactos firmados: módulos y paquetes de datos; geko-module-sign
    geko-server/             # HTTP común, montaje de los módulos, recarga de bundles
    geko-app/                # el host: binario y biblioteca, CLI, configuración, adaptadores, publicación
```

`geko-module-sdk`, `geko-fetch` y `geko-artifact` son las dependencias de git de los módulos, en la etiqueta `sdk-<contrato>`.

## 14. Hoja de ruta

1. **Host y módulos** (docs/MODULES_PLAN.md): pasos 1–6 *(hechos; quedan las semanas de uso real del paso 6)*; paso 7, los repositorios aparte *(hecho, 2026-10-22)*.
2. **Operación:** `BundleStore` con S3 y enrutador entre nodos; regeneración programada con diffs *(del geocoder)*.
3. **Más módulos:** teselas (`tiles`), enrutamiento.

## 15. Stack

Rust (edición 2024). Crates (cada uno solo en su crate): `ureq` (rustls, síncrono), `axum`, `tokio` (solo `geko serve`), `arc-swap`, `libloading`, `ed25519-dalek` (firmas), `sha2`, `serde`, `toml`, `clap`, `tracing`, `mimalloc`. Herramientas: `oras` para publicar artefactos firmados.

## 16. Pendiente

Al cerrar un punto, bórralo de aquí; si enseña algo, apúntalo en AGENTS.md. Lo de cada módulo está en su repositorio.

Esfuerzo: **S** = horas, **M** = una o dos jornadas, **L** = varias jornadas. Estado: ⬜ pendiente, ⏸️ aplazado.

| # | Punto | Motivo | Esfuerzo | Estado |
|---|---|---|---|---|
| 26 | Licencias antes de abrir el servicio | El aviso de base derivada (ODbL 4.6) está servido (`/v1/attribution`) | S | ⬜ Fuera del código: que lo revise un abogado y fijar `[serve] attribution_contact` |
| 43 | `BundleStore` con S3 y enrutador entre nodos | Hoy los bundles se sirven de disco o por HTTP (`bundles_url`) | M | ⬜ Cuando un nodo no baste |
| 46 | Módulos descargables | Plan en [MODULES_PLAN.md](MODULES_PLAN.md) | L | ⬜ Pasos 1–5 y 7 hechos; del 6, la suite hecha, y quedan unas semanas de uso real. ⏸️ Aplazado: `DylibModule` por defecto, elegido por la medición con España |
| 47 | Pruebas cruzadas | El golden del geocoder contra la última imagen del host de su contrato, y la conformidad del host contra el último módulo publicado (MODULES_PLAN.md, paso 7) | M | ⬜ |

## 17. Rutas

- `geko` lee siempre `/etc/geko/geko.toml` y guarda sus datos en `/var/lib/geko`: descargas (`raw/`), módulos bajados (`modules/`), lo de trabajo de cada módulo y los bundles publicados (`bundles/`).
- Para generar en una máquina y servir en otra, la que genera ofrece su `/var/lib/geko/bundles` por HTTP y la que sirve lo indica en `[serve] bundles_url`.
- `GEKO_CONFIG` y `GEKO_DATA` cambian las dos rutas, y `GEKO_MODULES` de dónde salen los módulos. Son para las comprobaciones y el desarrollo.
- Al cambiar el ámbito o los módulos, lo que ya no se pide se queda en `/var/lib/geko`, por si vuelve; se borra con `geko purge` (sección 3.1). Las versiones antiguas de los bundles las retira `[build] keep`.

## 18. Estado

- **Host y módulos** (2026-10-21/22, [MODULES_PLAN.md](MODULES_PLAN.md)): el host solo conoce los módulos por el contrato de `geko-module-sdk`. Cada módulo corre enlazado, como proceso propio o como biblioteca (`GEKO_MODULES`, `kind`), o se baja firmado de ghcr.io; la imagen publicada no lleva ninguno. Un proceso que cae se relanza aparte, con espera creciente, cuarentena de la consulta que lo tumba y plazo por llamada. Con el geocoder y España (2026-10-22), los datos salen idénticos a los de antes de los módulos; como proceso se pierde del 19 al 55 % de caudal con 16 a la vez, y como biblioteca, nada fuera del ruido.
- **Repositorios aparte** (2026-10-22): el geocoder (con los packs y los idiomas) en `candymaps/geko-geocoder` y `tiles` en `candymaps/geko-tiles`, con la SDK como dependencia de git.
- **Servicio:** página de demostración en `/`, `geko up`, CORS, claves de API, límite de peticiones, recarga en caliente y bundles por HTTP (`bundles_url`).

## 19. Comprobaciones

Todas viven en `ci/test.sh`, igual en local que en la CI (GitHub solo lo llama):

```bash
ci/test.sh [--variant code,contracts]
```

- `code`: clippy (los avisos son errores), las reglas de dependencias (`architecture`: qué puede usar cada crate y cada tecnología) y `cargo-deny` (licencias y avisos de seguridad).
- `contracts`: las pruebas y las suites de conformidad.

Se usan así:

- **En cada pull request, en la CI** (`ci.yml`): `ci/test.sh` entero si toca lo que compilan las comprobaciones (`ci/affected.sh`), nada si solo es documentación o lo que no compilan. El trabajo `test` y `protected.yml` son los estados que exige la protección de `main` y `dev`, con la rama al día, y `pr-comment.yml` comenta en la PR si pasaron, fallaron o no hacían falta. `protected.yml` falla una PR de cualquiera salvo el propietario (Dependabot solo puede subir las acciones de `.github/workflows/`) que cambie `LICENSE`, `CLA.md`, `CONTRIBUTING.md`, `README.md`, `AGENTS.md`, `CLAUDE.md`, `.github/`, `ci/` o lo que fija lo estrictas que son las comprobaciones (`deny.toml`, `clippy.toml`, `.cargo/`, `rust-toolchain.toml`); va en `pull_request_target`, que GitHub ejecuta tal como está en la rama base, para que la PR no pueda cambiarlo.
- **En cada push a `main`, en la CI** (`publish.yml`), que solo llega por una PR ya comprobada: sin repetir las comprobaciones, la imagen, si el push toca lo que entra en ella, y la etiqueta `sdk-<contrato>`, si cambió la SDK o lo que comparte con los módulos.
- **En local, a mano:** `make test` (todo) o `make test-changed` (lo que el cambio puede romper).
- **Los módulos** se comprueban en su repositorio: su golden y su rendimiento, con el host enlazado (`geko_app::main`).

## 20. Desarrollo y uso local

### 20.1 Con Docker

La imagen, `ghcr.io/candymaps/geko`, lleva el binario, ninguna configuración (sin un `geko.toml` montado se para y lo pide) y ningún módulo: baja los activos al volumen. Se monta la propia en `/etc/geko/geko.toml` (con `accept_licenses = true`; escucha siempre en `0.0.0.0:8080`) y un volumen en `/var/lib/geko`:

```bash
docker run -p 8080:8080 -v geko-data:/var/lib/geko -v ./geko.toml:/etc/geko/geko.toml ghcr.io/candymaps/geko
```

La orden por defecto es `up`: sirve enseguida (503 hasta el primer bundle) y genera en segundo plano lo que pide la configuración, y cada `[data] max_age_days` comprueba las fuentes y genera de nuevo lo que cambió, sin reiniciar. Con `serve` en vez de `up`, solo sirve lo que ya hay o lo que publica otra máquina (`bundles_url`).

### 20.2 Atajos: `make`

En Linux y macOS, `make <objetivo>`; en Windows, `make.cmd`, que lo ejecuta dentro de WSL. `make help` los lista:

- `make serve`, `make up` y `make data`: sirven, generan y sirven, o solo generan, con `/etc/geko/geko.toml` (`make install`, con `sudo`, instala antes el binario, `develop/geko.toml` si no hay ninguno, y `/var/lib/geko`). Sin `GEKO_MODULES`, bajan los módulos publicados.
- `make test` y `make test-changed`: las comprobaciones de `ci/test.sh`.
- `make image`: la imagen publicable con el binario de la última compilación. Se niega si el binario pide una glibc más nueva que la de la imagen (2.36).
- `make start-dev` (`develop/compose.yaml`, proyecto `geko-dev`): compila dentro de Docker (objetivo `local` del `Dockerfile`) geko y los módulos activos en su configuración, de sus repositorios junto a este (`../geko-<módulo>`; `DEV_MODULES_DIR=` los busca en otra carpeta), contra la SDK de este árbol, cambios sin commit incluidos, y vuelve a crear el contenedor. Los módulos van dentro de la imagen (`GEKO_MODULES`) y nunca se descargan; los packs de país son los del repositorio del geocoder. Sirve en `http://localhost:8080` (`DEV_PORT=`), con `develop/geko.toml` (`DEV_CONFIG=`). `DEV_MODULES=geocoder,tiles` fuerza qué módulos compilar. Sus datos van al volumen `geko-dev_data`; `make stop-dev` lo para.
- `make release`: compila, hace la imagen y la sube a `ghcr.io/candymaps/geko` (solo con todo en commit, tras `docker login ghcr.io`).
- **Publicación automática:** `publish.yml`, en cada push a `main`, publica la imagen `<versión>-<commit>`. Para sacar una versión basta subirla en `[workspace.package]`: el workflow añade `<versión>` y `latest` y crea la etiqueta `v<versión>`, con su release de GitHub.
