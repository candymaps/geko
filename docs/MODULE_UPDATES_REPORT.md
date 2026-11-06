# Actualización de módulos: estado actual y modelo propuesto

Fecha: 2026-11-06. Base: `dev` en `61c7488`, en la rama `feature/module-updates`. Analiza el código de `geko`, `geko-geocoder`, `geko-tiles` y `geko-router`, y lo publicado en `ghcr.io/candymaps` ese día. Las rutas de código son relativas a este repositorio, salvo las que nombran otro.

Las especificaciones siguen siendo `MODULES_PLAN.md` y `PROJECT.md`. Este informe describe lo que hace el código, y la sección 8 lista dónde se separa de ellas. La sección 9 propone el modelo acordado para sustituir el actual; las secciones 2 a 8 describen el sistema de hoy.

## 1. Resumen

- **Qué descarga el host:** tres módulos, `geocoder`, `tiles` y `router`. No descarga repositorios, sino paquetes OCI firmados. `geocoder` y `tiles` están activos por defecto; `router`, no.
- **Cuándo detecta una versión nueva:** solo al arrancar un proceso de `geko`. Un `geko up` en marcha no vuelve a mirar el registro.
- **Qué versión baja:** la que apunte la etiqueta `contract-<n>-<plataforma>`, que es **la última publicada**, no la más alta.
- **Fijar versiones:** sí, con `[modules.<m>] version`, solo con versión exacta y solo en descargas. En imágenes del modo operador y con `GEKO_MODULES` se ignora sin aviso.
- **Compatibilidad:** un entero (`CONTRACT`) comparado por igualdad, más la plataforma y la firma. No hay «host mínimo» ni rangos de contratos, aunque el plan los describe.
- **El problema principal:** el servidor exige que el módulo cargado sea *exactamente* la versión que generó el bundle, pero al arrancar elige el último publicado, sin mirar el bundle. Por eso **basta publicar un módulo y reiniciar para dejar el servicio sin datos** (P1, reproducido).
- **Propuesta:** cada versión del host fija la versión exacta de sus módulos, que van dentro de la imagen y se prueban antes de publicar. Desaparecen `CONTRACT`, las etiquetas de SDK y la descarga de módulos en ejecución (sección 9).

## 2. Qué se descarga

| Artefacto | Quién lo baja | Referencia | Destino |
| --- | --- | --- | --- |
| Geocoder | Host | `ghcr.io/candymaps/geko-geocoder` | `<data>/modules/geocoder/<versión>/` |
| Tiles | Host | `ghcr.io/candymaps/geko-tiles` | `<data>/modules/tiles/<versión>/` |
| Router (desactivado por defecto) | Host | `ghcr.io/candymaps/geko-router` | `<data>/modules/router/<versión>/` |
| Packs de país del geocoder | El módulo | `ghcr.io/candymaps/geko-geocoder/countries/<país>:format-<n>` | Los gestiona el módulo |
| Packs de país del router | El módulo | `ghcr.io/candymaps/geko-router/countries/<país>:format-<n>` | Los gestiona el módulo |

La lista de módulos obligatorios está en `ci/modules`. El host no tiene ninguna lista de módulos conocidos para descargar: construye la referencia a partir del nombre (`default_registry`), así que un módulo nuevo solo necesita publicarse.

Estado del registro el 2026-11-06, leído de forma anónima:

| Módulo | `contract-1-linux-x86_64` apunta a | Otras etiquetas | Tamaño de la biblioteca |
| --- | --- | --- | --- |
| geocoder | `0.3.0` | `0.1.0`, `0.1.0-ca215ca`, `0.2.0`, `0.3.0` | 18,2 MiB |
| tiles | `0.3.0` | `0.1.0-31b96e1`, `0.1.0-94f222b`, `0.2.0`, `0.3.0` | 6,2 MiB |
| router | `0.1.0` | `0.1.0` | 7,6 MiB |

Solo existe la plataforma `linux-x86_64`. Las imágenes del modo operador (`geko-<m>:latest`) todavía no están publicadas: las tres responden 404.

## 3. Cómo funciona hoy

### 3.1 Publicación de un módulo

Cada repositorio publica desde `.github/workflows/publish.yml` en cada push a `main` que toque su código:

1. Compila en `rust:<toolchain>-bookworm`, para no exigir una glibc más nueva que la de la imagen de geko.
2. Escribe `module.toml` (`name`, `version`, `contract`, `platform`) y firma el paquete con ed25519 (`signature.json`).
3. Sube con `oras push` **a la vez** las etiquetas `<versión>-<plataforma>` y `contract-<n>-<plataforma>`, con las anotaciones `geko.version`, `geko.contract` y `geko.platform`.

La versión depende de si el push es una release:

| Push a `main` | `geko.version` | ¿Mueve `contract-<n>`? |
| --- | --- | --- |
| Subió la versión de `Cargo.toml` (release) | `0.3.0` | Sí |
| No la subió | `0.3.0-<commit>` | **Sí** |

### 3.2 Resolución al arrancar

`lib.rs:346` abre todos los módulos activos en **cualquier orden de `geko`** que no sea `k8s` u `operator`: también `plan`, `serve`, `purge` y las órdenes de módulo. `Opener::open` (`modules.rs:160`) elige así:

1. **`GEKO_MODULES`:** el paquete local. No comprueba firma ni versión fijada.
2. **Módulo enlazado** en el binario (las pruebas de cada módulo), si no hay otra versión fijada.
3. **Con `GEKO_MODULES` definido:** error, nunca una descarga.
4. **Versión fijada:** `Store::exact` (`store.rs:139`).
5. **Sin fijar:** `Store::latest` (`store.rs:162`).

`Store::latest`:

1. Pide el manifiesto de `contract-<CONTRACT>-<plataforma>` (un GET, sin bajar la biblioteca).
2. Si no hay respuesta o estamos en `offline`, usa la copia más nueva del disco que sirva (`newest_kept`), con un aviso.
3. Comprueba contrato y plataforma en las anotaciones.
4. Si el digest coincide con `.digest` en la carpeta de esa versión, usa la copia, tras verificar de nuevo su firma.
5. Si no, baja a `.<versión>.partial`, verifica firma y `module.toml`, y cambia la carpeta.

`Store::exact` no consulta el registro si la versión ya está en disco: «una versión publicada no cambia».

### 3.3 Después de arrancar

La biblioteca queda cargada hasta que el proceso termina. `regenerate` (`run.rs:123`) reconstruye con esos mismos módulos, y solo por datos (`next_build`). **Ningún camino vuelve a mirar el registro de módulos.** La promesa de «sin reiniciar» se cumple para datos y bundles, no para código.

Cada `reload_secs` (30 por defecto), el servidor mira si hay otro bundle en `current`. `load_current` (`geko-server/src/lib.rs:174`) comprueba por cada módulo:

- Contrato distinto: error, y sigue el bundle anterior.
- Versión distinta y módulo descargado: `reopen` (`modules.rs:190`) **siempre devuelve error**, y sigue el bundle anterior.
- Versión distinta y módulo local o enlazado: solo un aviso, y carga el bundle.

## 4. Compatibilidad

| Momento | Qué comprueba | Dónde |
| --- | --- | --- |
| Antes de bajar | Anotaciones `geko.contract` y `geko.platform` (si faltan, pasa) | `compatible`, `store.rs:223` |
| Al bajar | Digest de cada capa, firma, tipo de artefacto y formato | `oci.rs`, `signature.rs` |
| Tras bajar y en cada arranque | `module.toml` firmado: nombre, versión, contrato, plataforma | `Package::check` |
| Al cargar | Saludo del adaptador con el contrato, y `describe()` | `dylib.rs`, `modules.rs` |
| Al cargar un bundle | Formato del manifiesto, contrato de la parte, versión exacta | `load_current` |
| En la CI del host | Existe `contract-<n>` de cada módulo de `ci/modules` | `ci/modules.sh check` |

Reglas de fondo:

- **`CONTRACT` es un único entero, hoy `1`.** Se compara por igualdad: un host habla un solo contrato.
- **Un cambio aditivo de la SDK no sube el contrato.** Mueve la etiqueta `sdk-<n>`, y los módulos compilan contra ella.
- **Consecuencia:** nada impide que un módulo nuevo, compilado con una SDK con más capacidades, se baje a un host antiguo del mismo contrato. El caso contrario sí se contempla en algún sitio (`Remote::progress`), pero no hay una regla general.
- **Con un contrato nuevo**, `contract-<n-1>` queda congelado: los hosts antiguos siguen funcionando, sin más arreglos.

## 5. Fijar versiones

```toml
[modules.geocoder]
version  = "0.3.0"                                # o "0.1.0-ca215ca"
registry = "ghcr.io/candymaps/geko-geocoder"      # opcional
```

- Solo admite una versión exacta. No hay rangos, canales ni «última de la 0.3».
- Si no existe o es de otro contrato, da error y nunca elige otra.
- Si está en disco, no toca el registro y funciona sin red.
- **No tiene efecto con `GEKO_MODULES`**, y por tanto tampoco en las imágenes del modo operador, sin ningún aviso. La versión ahí la decide `--tag`, una para todos los módulos, aunque sus versiones no coincidan (router va por la `0.1.0`).
- Fijar la versión es hoy también la única forma de evitar P1. Para actualizar hay que cambiar `geko.toml`, reconstruir y reiniciar, en ese orden y con cuidado: la sección 6 explica por qué.

## 6. Qué pasa cuando se publica una versión

Suponemos el bundle `v1`, generado por `0.3.0`, y que se publica `0.3.1`.

| Despliegue | Qué ocurre |
| --- | --- |
| `geko up`, sin reiniciar | Nada. Sigue en `0.3.0` indefinidamente, sin aviso de que hay otra. |
| `geko up`, reinicio sin fijar | Baja `0.3.1`. Rechaza `v1` y **da 503 hasta que acabe el build**. Si el build falla, sigue en 503. |
| `geko serve` aparte, reinicio sin fijar | Baja `0.3.1`, rechaza `v1` y **da 503 hasta que alguien publique un bundle de `0.3.1`**. |
| Generador en `0.3.1`, `serve` en marcha en `0.3.0` | Rechaza cada bundle nuevo y se queda con datos viejos hasta reiniciar. El error sale en `/health`. |
| Versión fijada en `0.3.0` | Nada cambia. |
| `offline` | Usa la copia más nueva del disco, sin mirar cuál generó el bundle. |
| Modo operador | No hay descarga. El pod nuevo carga el bundle de otra versión con solo un aviso. |
| Se publica una versión con un fallo | La recibe cada instalación sin fijar en su siguiente reinicio. Solo se corrige publicando otra. |

## 7. Problemas encontrados

Las marcadas como «reproducido» se probaron con el código real (sección 10).

### Graves

**P1. Un reinicio tras publicar un módulo deja el servicio sin datos.** Reproducido. Al arrancar se elige el último publicado, y `reopen` rechaza después el bundle en disco. El resultado:

```
/ready   503 {"bundle":null,"ready":false,"waiting":["a bundle"]}
/health  200 {"status":"ok","bundle":null,"bundle_error":"…changes its version only when geko restarts"}
```

El bundle sigue en disco, perfectamente servible con la versión que lo generó, que además suele estar en `<data>/modules/`. El error del mensaje engaña: reiniciar no lo arregla, porque es justo lo que lo causa. Y `/health` sigue en 200, así que un `HEALTHCHECK` no lo detecta.

**P2. La carga de un bundle no es atómica entre módulos.** Reproducido. `load_current` carga los módulos uno a uno y aborta en el primer error. Los anteriores ya tienen el bundle nuevo y los siguientes no; `loaded` no se actualiza, así que la operación se repite en cada tick:

```
tick 1: ok=false version=None fake loads=1 other loads=0
tick 2: ok=false version=None fake loads=2 other loads=0
tick 3: ok=false version=None fake loads=3 other loads=0
```

Con geocoder, router y tiles activos, eso son módulos sirviendo bundles distintos, y reabrir sus índices cada 30 segundos. Lo provoca tanto la versión como el contrato. El comentario de la función promete lo contrario: «on error the previous bundle stays in service».

**P3. Una versión mala llega a todos sin freno.** Cualquier push a `main` que cambie el módulo mueve `contract-<n>`, también sin subir la versión. No hay canales, despliegue gradual ni forma de retirar una versión; la marcha atrás es volver a publicar. Cada reinicio sin fijar es una actualización no pedida.

### Medios

**P4. Un fallo de descarga impide arrancar aunque haya copia válida.** `latest` solo usa la copia del disco si falla la petición del *manifiesto*. Si falla después (`store.rs:189`: una capa, la firma, `module.toml`) el error es definitivo, y cae todo `geko`, porque `lib.rs:346` exige todos los módulos. También ocurre con anotaciones de otro contrato o sin `geko.version`. El plan promete la copia en disco con el registro inalcanzable, y solo se cumple a medias.

**P5. `geko up` nunca se actualiza solo.** No hay comprobación periódica ni aviso, y ninguna orden dice «hay una versión nueva». Saber qué hay publicado exige reiniciar o ejecutar `geko plan`, que además la instala (P8).

**P6. Las versiones fijadas se ignoran sin aviso** con `GEKO_MODULES` y en el modo operador (sección 5). Quien fija una versión en producción cree tener una garantía que no tiene.

**P7. No hay compatibilidad de datos entre versiones.** La regla oscila entre dos extremos: igualdad exacta con módulos descargados, donde un arreglo sin cambios de formato obliga a reconstruir, y ninguna comprobación con módulos locales o en el modo operador, donde un cambio de formato puede cargarse sin más. Falta un formato de parte por módulo.

**P8. `geko plan` no es inocuo.** Baja y carga el módulo antes de mostrar nada, aunque `PROJECT.md` dice «sin hacer nada». Lo mismo vale para `geko purge modules`, que baja el último antes de limpiar.

**P9. Las claves de etapa no dependen de la versión del módulo.** Tiles y router usan constantes por etapa (`SCOPE_CODE`, `GRAPH_CODE`…) y el geocoder un `code` por fuente. Reutilizar etapas es bueno para reconstruir rápido, pero depende de que alguien suba a mano la constante. Si se olvida, el módulo nuevo publica datos generados por código antiguo con su propia versión en el manifiesto. No he auditado si eso ha pasado.

**P10. Falta el «host mínimo».** Lo describe el plan y no existe (sección 4).

**P11. El camino feliz de la descarga no tiene pruebas.** `store.rs` prueba `offline`, registro inalcanzable y copias sin firmar, pero ninguna prueba hace una descarga real. El registro local que anotaba el paso 4 de `MODULES_PLAN.md` no llegó a montarse. Cualquier cambio de este plan debería empezar por ahí.

### Menores

**P12. `by_version` ordena mal las compilaciones por commit.** Reproducido. Compara el hash como texto, o como número si solo tiene dígitos: `0.3.0-1234567` queda siempre por encima de `0.3.0-abc1234`, y `0.3.0-fff0000` por encima de `0.3.0-aaa0000`. Solo afecta a `newest_kept`, es decir, a `offline` y al registro caído, pero ahí puede elegir una versión más antigua.

**P13. Las versiones no son inmutables de verdad.** `exact` confía en que no cambian, mientras `latest` admite que cambie el digest de una versión ya bajada. Nada impide volver a subir la misma etiqueta, y dos máquinas pueden tener contenido distinto con la misma versión.

**P14. Sin credenciales de registro.** `OciClient` es anónimo, y trata 401, 403 y 404 como «no publicado», de modo que un fallo de permisos se presenta como que no existe. `[modules.<m>] registry` no sirve hoy para un registro privado.

**P15. Sin protección contra versiones anteriores.** La firma no caduca: quien pueda escribir en el registro no puede meter código propio, pero sí mover `contract-<n>` a una versión firmada antigua con un fallo conocido.

**P16. Dos procesos a la vez comparten `.<versión>.partial`.** Por ejemplo, un `geko plan` durante el arranque de `geko up`. No hay cerrojo en `Store`.

**P17. El modo operador no sigue los digest.** Usa `latest` por defecto, sin relación con lo que corre en cada pod, así que puede haber versiones mezcladas tras recrear unos pods y otros no. Además, `geko-router` no publica imagen ni tiene `refresh-image.yml`, de modo que no puede ejecutarse en ese modo.

**P18. Los módulos viejos no se limpian solos.** Se acumulan hasta `geko purge modules`. Con estos tamaños importa poco.

No es un problema hoy `MAX_BLOB` (256 MiB, y lo más grande son 18,2 MiB), aunque cada artefacto se baja entero a memoria.

## 8. Dónde difieren la documentación y el código

| Afirmación | Dónde | Realidad |
| --- | --- | --- |
| `geko serve` usa la versión que generó el bundle, y la baja si falta | `PROJECT.md:40`, `MODULES_PLAN.md`, `README.md` | Usa la última o la fijada, y rechaza el bundle |
| Al pasar a `current`, el servicio cambia al módulo del bundle en la misma recarga | `MODULES_PLAN.md` | `reopen` siempre falla con módulos descargados, como decidió el mismo plan el 2026-10-23 |
| Si la versión fijada difiere del bundle, aviso en el log | `MODULES_PLAN.md` | Error, y no carga el bundle |
| El host declara varios contratos (p. ej. 2–3) | `MODULES_PLAN.md` | Un solo `CONTRACT`, por igualdad |
| Se comprueba el host mínimo | `MODULES_PLAN.md` | No existe |
| El error lista las versiones compatibles | `MODULES_PLAN.md` | Solo propone quitar `version` |
| Con el registro inalcanzable, la copia en disco | `MODULES_PLAN.md` | Solo si falla el manifiesto (P4) |
| La versión del módulo va en los sellos de las etapas | `MODULES_PLAN.md`, paso 3 | Constantes manuales (P9) |
| `geko plan` no hace nada | `PROJECT.md:71` | Baja y carga los módulos (P8) |
| geko se mantiene actualizado sin reiniciar | `AGENTS.md` de `geko-context` | Cierto para datos, no para módulos |

`Slot`, `Reopen` y `a_bundle_of_another_version_of_the_module_switches_to_it` son piezas de cambio en caliente que quedaron de `ProcessModule`: están probadas, pero el host real nunca las usa con éxito.

## 9. Modelo propuesto: cada host fija sus módulos

Acordado el 2026-11-06, tras este análisis. Sustituye a la idea de resolver versiones en cada instalación: casi todos los problemas de la sección 7 vienen de combinar host y módulos al arrancar. La combinación pasa a decidirse al publicar el host.

### Principios

- **Solo se elige la versión del host.** Cada versión del host fija la versión exacta de cada módulo, y no se puede cambiar.
- **Los módulos van dentro de la imagen.** Los tres suman unos 32 MiB, y Docker es la única distribución. En ejecución no se descarga ningún módulo.
- **La compatibilidad de código se comprueba antes de publicar,** no en cada instalación. Por eso no hacen falta `CONTRACT` ni etiquetas de SDK.
- **La compatibilidad de datos se mantiene,** porque bundles y packs no se publican con el host.

### Flujo

1. Cada módulo publica su paquete firmado, como ahora, sin la etiqueta `contract-<n>`.
2. El repositorio del host guarda en un fichero la versión y el digest de cada módulo. Un PR automático lo actualiza cuando sale una versión de un módulo.
3. Al construir la imagen, se bajan esos paquetes, se verifican firma y digest, y se copian dentro. geko los lee de `GEKO_MODULES`.
4. La CI del host carga cada `.so` real, pasa la suite de conformidad y hace un build y un `serve` de Luxemburgo. Si algo falla, no hay release.

Se usan los binarios publicados, y no una compilación en el host, para distribuir los mismos bytes que pasaron las pruebas de cada módulo y para que la release no compile nada. A cambio, un desajuste en la frontera de la biblioteca no lo ve el compilador, porque por ella solo pasan bytes: por eso la prueba del paso 4 debe ejercitar el módulo, incluido el build, que la suite de conformidad no toca.

### La SDK

El crate `geko-module-sdk` se queda, con `geko-fetch` y `geko-artifact`: son la interfaz que implementan los módulos, y sin ellos no compilan. Siguen en este repositorio, pero con releases propias:

```toml
geko-module-sdk = { git = "https://github.com/candymaps/geko.git", tag = "sdk-v0.4.0" }
```

- **Versión propia:** hoy comparten la del workspace del host. Pasan a tener una independiente.
- **Etiquetas inmutables:** `sdk-v<versión>` sustituye a `sdk-<n>`, que hoy se mueve con `git tag -f`. Se crea al subir la versión de la SDK.
- **El orden evita dependencias circulares:** primero sale la SDK, luego los módulos la adoptan y publican, y por último el host fija esos módulos. Una etiqueta de release del host no serviría, porque los módulos deben compilarse antes de que esa release exista.
- **Solo se usa al compilar:** en ejecución no hay comprobación de compatibilidad. La garantiza la prueba de la CI del host.
- **Para probar sin publicar,** `make start-dev` ya compila con la SDK del árbol.

### Qué se elimina y qué se queda

| Se elimina | Se queda |
| --- | --- |
| `CONTRACT`, el saludo con contrato y `contract` en bundles y `module.toml` | El crate `geko-module-sdk` |
| Etiquetas `contract-<n>` y `sdk-<n>` (esta, sustituida por `sdk-v<versión>`), y `ci/modules.sh check` | Los paquetes firmados de los módulos, verificados al construir la imagen |
| `store.rs`, y `[modules.<m>] version` y `registry` (con un error claro si siguen en `geko.toml`) | Los packs de país, su `format-<n>` y su firma |
| `geko purge modules` | `GEKO_MODULES`, para la imagen y para desarrollo |
| Las imágenes por módulo y `refresh-image.yml` | El formato del manifiesto del bundle |

Se pierde que un arreglo de un módulo llegue sin release del host, y que se puedan instalar módulos de terceros desde un registro: solo por `GEKO_MODULES`, montando una carpeta. Un módulo desactivado ocupa disco, aunque no se carga.

### Fases

1. **Comprobar la combinación.** Fichero de versiones y trabajo de CI del paso 4, sin cambiar nada más. Medir cuánto tarda.
2. **Arreglar la carga de bundles.** Un número de formato de datos por módulo sustituye a la igualdad exacta de versión, en todos los modos, y la carga pasa a ser en dos fases: se comprueban todos los módulos antes de cargar ninguno, y un bundle rechazado no se reintenta en cada recarga. `/ready` y `/health` dejan claro cuándo no hay bundle. Debe ir antes que la fase 3: sin ella, actualizar el host da 503 hasta terminar el build.
3. **Módulos en la imagen.** Una sola imagen, también para el operador (`--image`), y fuera las imágenes por módulo.
4. **Borrar.** `store.rs`, las opciones de `[modules.<m>]`, `CONTRACT`, las etiquetas `contract-<n>` y `sdk-<n>`, y `ci/modules.sh check`. La SDK estrena versión propia, y los módulos pasan a `tag = "sdk-v<versión>"`.
5. **Documentación.** `PROJECT.md`, `MODULES_PLAN.md`, `README.md`, `jekyll/` y, en `geko-context`, `AGENTS.md` y `docs/architecture.md`.

### Qué pasa con cada problema

| Problemas | Resultado |
| --- | --- |
| P1 | Desaparece al reiniciar. Al actualizar el host, lo resuelve el formato de datos (fase 2) |
| P3, P5, P6, P8, P10, P12, P13, P15, P16, P17, P18 | Desaparecen: solo hay una versión posible, fijada por digest y probada antes |
| P4 | Desaparece para módulos. El host no los descarga |
| P11 | Sustituido por la prueba de la fase 1 |
| P2, P7 | Se arreglan en la fase 2 |
| P9 | Sigue abierto: decidir entre invalidar etapas por versión o comprobar las constantes en la CI |
| P14 | Sigue para los packs de país, que se bajan de forma anónima |

### Pendiente de decidir

- **`[build] keep`:** con `1` no se puede volver al bundle anterior tras actualizar el host; con `2` sí, a costa de disco.
- **Automatización del fichero de versiones:** qué dispara el PR y si la release del host es automática o la lanza alguien.
- **Enlazar los módulos en un solo binario:** queda fuera de este plan. Quitaría `wire`, `Remote` y `dylib`, pero exige medir y revisar la regla de que el host no depende de ningún módulo.

### Deseable, sin planificar

**Extraer la SDK a un repositorio `geko-sdk`.** No forma parte de este plan, ni como paso final, y no tiene fecha. Daría etiquetas normales (`v0.4.0`), versión independiente sin tocar el workspace del host y una dependencia más clara: host y módulos dependerían de `geko-sdk`, y ningún módulo del host. Es viable, porque `geko-module-sdk`, `geko-fetch` y `geko-artifact` no dependen de ningún crate del host. El freno es que un cambio en la SDK y el host pasaría de un PR a dos y una release, y eso es hoy lo habitual: 5 de los 6 commits que tocaron esos crates desde el 2026-10-22 tocaron también crates del host. Conviene retomarlo si esa proporción baja.

## 10. Cómo se comprobó

- **Lectura:** `store.rs`, `modules.rs`, `lib.rs`, `run.rs`, `dylib.rs`, `remote.rs`, `bundle.rs` y `config.rs` de `geko-app`; `geko-server/src/lib.rs`; `geko-artifact`; `geko-fetch/src/oci.rs`; `ci/modules.sh` y `publish.yml` de cada repositorio.
- **Registro:** etiquetas, anotaciones y tamaños leídos con el token anónimo de `ghcr.io`.
- **Reproducciones:** tres pruebas temporales, ejecutadas con `cargo test` en WSL y retiradas después, sin commit:
  - P1: `BundleManager` con un módulo `2.0.0`, un bundle generado por `1.0.0` y un `Reopen` que falla con el mismo mensaje que `Opener::reopen`.
  - P2: dos módulos en el orden de montaje, con el segundo rechazado, y tres `refresh`.
  - P12: `by_version` sobre pares de versiones con commit.
- **Sin probar:** no se ejecutó geko de extremo a extremo contra el registro. P1 usa un doble del `Reopen` real, y lo que ocurre en `geko up` y en el modo operador sale de la lectura del código.
