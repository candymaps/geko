# Módulos

Cada servicio de geko (hoy `geocoder`; después `tiles` y otros) es un **paquete que el host descarga y carga en tiempo de ejecución**, desde su propio repositorio, en vez de compilarse dentro del binario. El host lo baja de los packages de su repositorio cuando `geko.toml` lo activa (`[modules] geocoder = true`). Lo pendiente del contrato está en PROJECT.md §16 (puntos 46 y 47); el plan de cada módulo, en su repositorio.

## Reparto

| | Host (`candymaps/geko`) | Módulo (`candymaps/geko-<módulo>`) |
|---|---|---|
| Código | CLI, configuración, ámbito (códigos ISO), `Downloader`, licencias aceptadas, publicación y recarga de bundles, HTTP común (`/health`, `/metrics`, claves de API, CORS, límites, atribución), registro de módulos y descarga de artefactos firmados | Sus fuentes, sus etapas de generación, su parte del bundle, sus rutas (`/<módulo>/…`), su demo, sus órdenes de depuración. El geocoder: fuentes (OSM, Overture, OGC), fusión, índice, inversa, motor de búsqueda, caché de respuestas |
| Puertos | `Module` (el contrato, en la SDK) | Los suyos, con sus suites de conformidad. El geocoder: `SourceProvider`, `TextNormalizer`, `AddressParser`, `GeocodeIndexBuilder`, `GeocodeEngine`, `ReverseIndex` |
| Tipos | Los de la SDK: país, región, ámbito, licencia, petición de descarga, artefacto, petición y respuesta HTTP, configuración opaca | Los suyos. El geocoder: `Hit`, `ParsedQuery`, `QueryContext`, `Coord`, `PublicId`… |
| Datos | Ninguno | Sus paquetes de datos. El geocoder: los packs de país (`countries/`, con `golden/`) y los idiomas (`languages/`) |
| Documentación | PROJECT.md y este documento | Los suyos: su README (con todas sus opciones), su AGENTS.md y, en el geocoder, `FUSION_METHOD.md` |

Regla: **si un tipo, un dato o un puerto solo tiene sentido para un módulo, vive en el módulo.** El host no sabe qué es un pack: el módulo baja los paquetes de datos que necesita para el ámbito con el mecanismo genérico de artefactos firmados de la SDK, el mismo con que el host baja los módulos, así que el host no necesita conocer su destino ni su formato.

## La SDK (el contrato)

La define y la publica el host, en su propio repositorio: el puerto pertenece a quien lo consume. Son tres crates, `geko-module-sdk` (el contrato), `geko-fetch` (el `Downloader`) y `geko-artifact` (artefactos firmados). GitHub Packages no tiene registro de Cargo, así que se publican como etiqueta de git del repositorio del host, `sdk-<contrato>`, que `publish.yml` mueve cuando cambian, y cada módulo los usa como dependencias de git:

```toml
geko-module-sdk = { git = "https://github.com/candymaps/geko.git", tag = "sdk-1" }
```

`Module` (`crates/geko-module-sdk/src/module.rs`, con su versión en `CONTRACT`):

- `describe`: nombre, versión, contrato, rutas HTTP que atiende (método, patrón relativo, si van al pool de búsqueda y si son públicas sin clave de API), su `openapi.yaml`, lo que añade a `/v1/attribution` y sus órdenes.
- `plan` y `fetch`: del ámbito y su configuración a los datos que hacen falta, descargados con el `Downloader` del host (caché, `offline`, licencias, descargas en paralelo).
- `prepare`: lo que necesitan las órdenes que generan (el geocoder baja ahí los packs del ámbito).
- `build`: sus etapas por país, con sus sellos (PROJECT.md §8) en su propio directorio de trabajo, y las partes del bundle que deja, sus licencias, sus metadatos y sus etapas para el informe. El host las publica como ficheros opacos (ruta, hash, tamaño), y el manifiesto del bundle (formato 2) guarda de cada módulo su nombre, versión, contrato y metadatos. Después, `published` (el geocoder borra las descargas) y `next_build` (cuándo volver a generar en `geko up`).
- `load(bundle)`, `handle(Request) -> Response` (síncrono: el host monta axum, aplica acceso y límites, y llama desde su pool), `metrics(&mut String)`, `shutdown`.
- `command(args)`: órdenes del módulo (`geko geocoder query …`), reenviadas tal cual.
- `purge`: sus datos que la configuración ya no pide (`geko purge data`) y sus paquetes de fuera del ámbito (`geko purge packages`).
- `unavailable()`, del lado del host: si el módulo no puede responder, y por qué (`/health`, `modules_unavailable`).

**Rutas bajo el nombre del módulo.** El módulo declara sus rutas relativas (`v1/search`) y el host las monta siempre bajo `/<módulo>/`: dos módulos no pueden pisarse, y uno nuevo no toca las rutas de los demás. El host se queda sin prefijo con lo común: `/health`, `/metrics`, `/v1/attribution` (las licencias de todo el bundle, más lo que añade cada módulo), `/openapi.yaml` (solo las rutas del host) y `/docs` (Swagger con la del host y la de cada módulo). `/` lleva a la demo del primer módulo activo que tenga una.

Un cambio del módulo (ranking, formato del índice, fuente nueva, ruta nueva, opción nueva, métrica, orden de depuración, datos del pack) **no toca el host**. Solo lo toca una capacidad nueva del contrato (streaming, WebSockets, tareas de fondo, un servicio nuevo del host) o un cambio incompatible de la SDK. Los cambios del contrato son aditivos siempre que se pueda: campos opcionales y capacidades que se consultan en `describe` antes de usarlas. Un cambio que lo rompe sube `CONTRACT`, y cómo se sube con los módulos está en AGENTS.md («Key rules»).

**Conformidad.** `geko_module_sdk::conformance::module::check` (*feature* `conformance`), sobre un módulo abierto con el ámbito vacío y sin bundle: que `describe` lleve el contrato de la SDK, una versión y rutas relativas sin repetir (con `{nombre}` como parte entera), que su `openapi` sea una de sus rutas GET, órdenes sin repetir y un error para una orden desconocida, ningún progreso fuera de un build y `/metrics` en el formato de Prometheus, bajo `geko_`. Cada ruta, con peticiones raras (vacía, valores imposibles, 10.000 letras, nulos, un id con `..`, un cuerpo roto), se responde sin 500, y un error es `{"error": …}` en JSON; igual tras cargar un bundle sin su parte, y desde 8 hilos a la vez. La pasa cada módulo (`module/tests/conformance.rs`) y, en `geko-app`, el módulo de prueba enlazado y por la biblioteca. Un requisito nuevo del contrato va a la suite.

## Configuración

Cada módulo tiene su propio fichero, `/etc/geko/geko-<módulo>.toml`, junto al `geko.toml` (con `GEKO_CONFIG`, en la carpeta de ese fichero), o el que diga `[modules.<módulo>] config`. `geko.toml` se queda solo con lo del host: qué módulos se activan y de dónde se bajan, el ámbito, las descargas, los bundles y el HTTP común.

- El host lee el fichero del módulo solo si el módulo está activo (un módulo desactivado no abre ficheros) y se lo pasa sin interpretarlo. Si no existe, el módulo usa sus valores por defecto. Lo valida el módulo, con errores que nombran el fichero y la clave.
- El ejemplo con todas las opciones vive en el repositorio del módulo, que las documenta (el geocoder: `develop/geko-geocoder.toml` y su README). Una opción nueva de un módulo no toca ni el host ni `geko.toml`.
- Las opciones que afectan a la generación entran en los sellos de las etapas del módulo.
- Una opción que el host ya no conoce y sigue en `geko.toml` (p. ej. `[serve] search_budget_ms`, que pasó al geocoder) da un error al arrancar que dice a qué fichero se ha movido, en vez de ignorarse: ignorarla cambiaría en silencio el comportamiento tras actualizar, y `geko.toml` ya rechaza las claves que no conoce (`deny_unknown_fields`).

## Adaptador: `DylibModule`

El host solo maneja `Arc<dyn Module>`. Un módulo se enlaza en el binario (`geko_app::main(linked)`, solo para las comprobaciones del propio módulo: el golden, valgrind) o se carga como biblioteca, `DylibModule`, el único adaptador de un paquete (`kind = "dylib"` en su `module.toml`).

- La `.so` exporta con `geko_module_sdk::export_module!` cinco funciones `extern "C"` (`geko_module_hello`, `_open`, `_call`, `_free`, `_close`) que pasan mensajes serializados (`geko_module_sdk::wire`): tramas con la longitud de la cabecera y de la carga (`u32`), la cabecera en JSON y la carga en bruto. El primer mensaje es el saludo con el contrato. La frontera pasa bytes, sin tipos de Rust, así que el host y cada módulo se compilan cada uno con su versión de Rust.
- Cada función envuelve su trabajo en `catch_unwind` (un pánico que cruzara la frontera abortaría el proceso): un pánico es un error de esa llamada, un 500 en una petición. Cada lado libera su memoria (`geko_module_free`).
- El módulo se compila como `.so` en un crate aparte (`module/dylib`, `libgeko_<módulo>.so`), con su propio mimalloc y su `tracing`, para que enlazarlo en el host no compile también la `cdylib`. El host lo abre con `libloading` y no lo descarga nunca.
- A cambio de no costar nada: un fallo grave del módulo (segfault, desbordamiento de pila, aborto, quedarse sin memoria) tira geko, que comparte proceso con él; cambiar de versión de un módulo exige reiniciar, y el pico de memoria del build se queda en el proceso de geko.

Decidido (2026-10-23), con la medición A/B con España (2026-10-22, 16 peticiones a la vez, el mismo bundle y en turnos): como proceso propio con un socket Unix (`ProcessModule`), del 19 al 55 % menos de caudal (búsqueda −19 %, autocompletado −20 %, inversa −55 %) y 2,9 s de apertura; como biblioteca, nada fuera del ruido y 0,5 s. `ProcessModule` y su servidor en la SDK se quitaron; están en la historia de git (`git show 42e336d:crates/geko-app/src/process.rs`).

## Versiones y compatibilidad

- **El host fija el contrato, no la versión del módulo.** Cada publicación de un módulo lleva las etiquetas `<versión>-<plataforma>` y `contract-<n>-<plataforma>` (el último de ese contrato), y las anotaciones `geko.version`, `geko.contract` y `geko.platform`. Con el módulo activo, el host baja el último del contrato que habla: un arreglo de un módulo llega sin release del host.
- **Versión fijada, opcional:** `[modules.<módulo>] version = "1.4.2"` (producción, volver atrás). También `[modules.<módulo>] registry`, por defecto `ghcr.io/candymaps/geko-<módulo>`.
- **Comprobaciones,** nunca eligiendo otra versión en silencio: antes de bajar nada, las anotaciones del manifiesto OCI (`OciClient::annotations`), que rechazan otro contrato u otra plataforma; tras bajarlo, la firma y el `module.toml` firmado (`name`, `version`, `contract`, `platform`); al abrirlo, el saludo. Una versión ya bajada no se vuelve a pedir; la última del contrato, solo si cambió su digest; sin registro u `offline`, la más nueva de las del disco que sirvan. Errores claros para: ningún módulo del contrato, versión fijada inexistente, firma inválida, `offline` sin el módulo.
- **El bundle fija la versión con la que se sirve.** El manifiesto guarda la versión exacta del módulo que generó cada parte, y `geko serve` usa esa y la baja si falta (es código, no fuentes: no rompe «serve nunca descarga fuentes»). Si una parte la generó otra versión, `BundleManager` la abriría y cambiaría en caliente; con `DylibModule` eso no es posible sin reiniciar: error, se sigue con el bundle cargado y `/health` lo dice (`bundle_error`). Un módulo enlazado o de `GEKO_MODULES` sirve tal cual, con un aviso en el log.
- **La versión de un módulo publicado** es `<versión>-<commit>` (o `<versión>` en una release), compilada en él con `GEKO_MODULE_VERSION` (`module_version!` de la SDK): el bundle dice la versión exacta que lo generó.
- **Publicación del host:** `ci/modules` lista los módulos que deben estar publicados para el contrato de geko, y `ci/modules.sh check` lo comprueba en la CI de las pull requests que cambian el contrato o esa lista (ni `dev` ni `main` aceptan un contrato que un módulo aún no habla) y en `publish.yml`, antes de la imagen y la release. Dejar de soportar un contrato es versión mayor del host, anunciada.
- `geko plan` muestra la versión de cada módulo y de dónde sale.

## Descarga y verificación

`geko-artifact` es el crate de artefactos firmados, que usan el host y, por la SDK, los módulos: OCI con etiquetas, firma ed25519 contra `TRUSTED_KEYS` con un `Kind` por clase de artefacto (su dominio va en el mensaje firmado: un pack no pasa por módulo) y hash de cada fichero, copia en disco si el registro falla, `offline`, copias a mano sin `.digest`, directorio local sin descarga y `purge`. Sirve para:

- **Módulos**, por plataforma (`linux-x86_64`), en `/var/lib/geko/modules/<módulo>/<versión>/`. `GEKO_MODULES` usa los locales y nunca descarga. Solo se ejecuta código firmado, salvo el de ese directorio local.
- **Paquetes de datos de un módulo** (los packs de país del geocoder), que baja el propio módulo dentro de su directorio. Su formato es cosa del módulo.

`geko purge modules` borra los módulos que ya no piden ni la configuración ni ningún bundle conservado, y `geko purge packages` los paquetes de datos de fuera del ámbito.

**Publicación de un módulo,** desde su repositorio: `ci/modules.sh build` compila su `module/` (`module.toml` y el crate `geko-module-<módulo>-dylib`) en `rust:<toolchain>-bookworm`, por la glibc de la imagen del host, y completa el `module.toml`; `ci/modules.sh publish` lo firma con `geko-module-sign` (de `geko-artifact`, con la clave `GEKO_PACK_SIGNING_KEY`) y lo sube a `ghcr.io/candymaps/geko-<módulo>`.

**Imagen de Docker del host sin módulos.** `ghcr.io/candymaps/geko` es solo el host; los módulos no son imágenes de Docker, sino artefactos OCI, como los packs. Al arrancar, baja al volumen los que estén activos en `geko.toml`; uno desactivado no se baja. En los arranques siguientes usa la copia del volumen, y con el registro inalcanzable arranca con ella. Sin copia y sin red, o con `offline` y sin el módulo, no arranca y lo dice. La imagen no cambia al publicar un módulo: solo con el código del host. `make start-dev` compila los módulos de sus repositorios, junto al del host, y los lleva dentro (`GEKO_MODULES`).

## Decidido

- 2026-10-21: la imagen del host no lleva módulos y baja los activos al arrancar; la SDK se publica en el repositorio del host, como dependencia de git con etiqueta `sdk-<n>`, no desde un registro de Cargo; los idiomas y los packs son del geocoder (`tiles` no traduce), y los idiomas siguen compilados dentro de su binario, no como paquete de datos.
- 2026-10-23: `DylibModule` es el único adaptador (arriba).
