# Plan de actualización de módulos

Estado: fase 1 hecha, salvo el token del paso 1.7 y su prueba en GitHub; fase 2 hecha en el host, a falta de publicar geocoder y router (2026-11-06). Rama: `feature/module-updates`. El análisis y los motivos están en [`MODULE_UPDATES_REPORT.md`](MODULE_UPDATES_REPORT.md).

**Objetivo:** solo se elige la versión del host, y ella decide exactamente qué módulos lleva. La compatibilidad de código se comprueba al publicar, y cada módulo funciona de forma independiente.

⏸ marca una parada: alguien tiene que fusionar y publicar antes de seguir.

## Fase 1: comprobar la combinación (host)

- [x] 1.1. Crear `modules.lock` en la raíz, con versión y digest de geocoder `0.3.0`, tiles `0.3.0` y router `0.1.0`.
- [x] 1.2. Crear `ci/modules.sh pull <carpeta>`, que baja cada paquete por digest y verifica firma y versión.
- [x] 1.3. Crear la prueba que carga cada `.so` real y pasa la suite de conformidad.
- [x] 1.4. Crear la prueba de humo: build y `serve` de Luxemburgo con los tres módulos.
- [x] 1.5. Crear el workflow `modules.yml`, para los PR que cambien `modules.lock` o la SDK.
- [x] 1.6. Hacer que `publish.yml` exija ese mismo trabajo.
- [ ] 1.7. Crear un workflow programado que compare el registro con `modules.lock` y abra el PR a `dev`. Escrito (`modules-update.yml`); falta el secreto `GEKO_MODULES_PR_TOKEN`, y sin él avisa y no hace nada.
- [x] 1.8. Medir cuánto tarda.

Medido el 2026-11-06, en WSL, sin compilar geko desde cero:

| Parte | Tiempo |
| --- | --- |
| `pull` | 19 s |
| `conformance` | 5 s |
| `smoke`, con las descargas ya hechas | 92 s, de ellos 56 s de Planetiler |

Sin medir: la primera ejecución con todas las descargas (los polígonos de `osmdata` son 1,8 GB), porque `osmdata.openstreetmap.de` estaba caído. La prueba de humo se ejecutó con los polígonos del golden de Baleares, de modo que Luxemburgo salió sin agua ni tierra. Tampoco han corrido los workflows en GitHub: solo los ha validado `actionlint`.

Comprobado que falla cuando debe: con una `.so` manipulada, un digest falso y una versión que no coincide.

La prueba de humo reutiliza las descargas de la última ejecución buena, en modo `offline`, porque `geko` consulta las fuentes aunque tenga la copia: sin eso, una fuente caída bloquearía una release.

## Fase 2: módulos independientes

- [x] 2.1. Geocoder: validar el formato de sus datos en `load`. No hace falta cambiar nada: `geocode-fst` y `reverse-rstar` ya rechazan otro formato al abrirse, y `load` no cambia lo servido hasta abrir todos los segmentos. Esa rama no tiene prueba propia.
- [x] 2.2. Router: lo mismo. Con `preload = true` ya lo hacía; con `preload = false` no miraba el fichero hasta la primera petición. Arreglado en `dev` del router (PR #13).
- [ ] 2.3. ⏸ Publicar el router, y actualizar `modules.lock`. El geocoder no cambia, así que no hace falta publicarlo.
- [x] 2.4. Host: dejar de comparar versiones al cargar un bundle.
- [x] 2.5. Host: carga, estado y errores por módulo, sin reintentar un bundle rechazado.
- [x] 2.6. Host: `/health`, `/ready` y atribución por módulo.
- [x] 2.7. Host: `[build] keep = 2` por defecto.
- [x] 2.8. Host: build independiente, en un PR aparte.

Pasos 2.4 a 2.8 hechos sin PR, por decisión del 2026-11-06: **el host no se publica antes del paso 2.3**, porque deja en manos de cada módulo validar sus datos. El build independiente va en su propio commit, para poder separarlo.

Decidido al hacerlos:

- `CONTRACT` se sigue comparando hasta el paso 4.2, pero ya solo rechaza al módulo afectado.
- `/ready` sigue diciendo si puede responder todo el proceso, y `/ready?module=<m>` pregunta por uno solo.
- `geko serve` escucha desde el principio y carga en segundo plano, con todos los módulos a la vez. Hasta que un módulo carga, responde 503 en sus rutas.
- Un build fallido conserva solo la parte de los módulos que fallaron, no la de los desactivados. Sus ficheros y licencias siguen hasta el siguiente build sin fallos, porque el manifiesto no dice de quién son.

## Fase 3: módulos en la imagen

- [x] 3.1. `Dockerfile`: copiar a la imagen los paquetes de `modules.lock`, ya verificados. Lo hace la etapa `modules` con `ci/modules.sh pull`. La imagen pesa 212 MB, de ellos 33 MB de módulos. Comprobada en local: arranca, carga los tres por `GEKO_MODULES` y responde. Cambiar `modules.lock` ya publica imagen.
- [x] 3.2. Operador y manifiestos: una sola imagen por defecto, y revisar `geko-minik8s`. Desaparecen `--image-prefix` y `--operator-image`: queda `--image`, para el operador y para cada módulo.
- [ ] 3.3. ⏸ Publicar el host con esa imagen y comprobarla en kind.
- [ ] 3.4. Geocoder y tiles: quitar `ci/image.Dockerfile`, `refresh-image.yml` y los pasos de imagen.

## Fase 4: borrar lo que sobra

- [x] 4.1. SDK: darle versión propia en sus tres crates, y que `publish.yml` cree `sdk-v<versión>`. Empieza en `2.0.0`. La etiqueta no se mueve: si el SDK cambia sin subir la versión, `publish.yml` falla.
- [x] 4.2. Quitar `CONTRACT`: saludo, `Description`, bundles y `module.toml`. El host ya no comprueba nada, pero `BundleModule` sigue escribiendo `"contract": 1`, porque los módulos publicados lo exigen al leer el manifiesto (paso 4.8).
- [x] 4.3. Quitar `store.rs`, `geko purge modules`, y `[modules.<m>] version` y `registry`, con un error claro si siguen en `geko.toml`.
- [x] 4.4. Quitar `ci/modules`, `ci/modules.sh check` y sus pasos en `ci.yml`, `publish.yml` y `pr-comment.yml`.
- [ ] 4.5. ⏸ Publicar el host, que crea la primera `sdk-v<versión>`.
- [ ] 4.6. Los tres módulos pasan a `tag = "sdk-v<versión>"` y dejan de mover `contract-<n>`. Hay que quitar `contract` de `Description` y de sus `BundleModule`, y dejar de usar `geko-module-sign contract` y `CONTRACT` en `ci/modules.sh`. El geocoder, además, `version` de `Linked`.
- [ ] 4.7. ⏸ Publicar los módulos y actualizar `modules.lock`.
- [ ] 4.8. Quitar `WrittenModule` de `bundle.rs`, que escribe el `contract` de los módulos antiguos, y subir la versión del SDK.

## Fase 5: documentación

- [ ] 5.1. `PROJECT.md`, `MODULES_PLAN.md`, `README.md` y `jekyll/`.
- [ ] 5.2. `AGENTS.md` de cada repositorio.
- [ ] 5.3. `AGENTS.md` y `docs/architecture.md` de `geko-context`.

## Antes de empezar

- **No puede ir todo del tirón:** hay 4 paradas (2.3, 3.3, 4.5 y 4.7), porque esos pasos publican en `main`. Cada PR queda listo, y se sigue cuando esté publicado. Entre paradas se puede avanzar seguido.
- **El paso 1.7 necesita un token.** GitHub no lanza las pruebas en los PR que abre el token por defecto de Actions, así que hará falta un token personal o una GitHub App como secreto del repositorio. Si aún no existe, ese paso se deja para el final.
- **Las etiquetas ya publicadas se quedan:** `contract-1` y `sdk-1` no se borran, porque las usan los hosts instalados y los commits antiguos de los módulos. Solo dejan de moverse.
- **El orden de las publicaciones es parte del plan:** los paquetes actuales siguen funcionando hasta 4.7. Hasta ese paso, el host aún tiene que aceptar los módulos publicados con contrato.
- **La fase 4 no entra en `dev` hasta que 3.3 esté publicado:** si no, saldría a `main` con él.
- **Entre 4.2 y 4.6, `make start-dev` y el job `operator` no compilan los módulos:** compilan su código contra el SDK de este árbol, y ese código aún escribe `contract`. Los módulos publicados no se ven afectados.

## Decisiones

- Publicar sigue siendo fusionar a mano el PR de `dev` a `main`.
- Los módulos no se compilan en el host: siguen siendo `.so` separadas, publicadas y firmadas desde su repositorio, y la CI del host prueba esos mismos binarios.
- La SDK se queda en este repositorio, con etiquetas inmutables `sdk-v<versión>`.
- `modules.lock` va en la raíz: `protected.yml` solo deja al propietario cambiar `ci/`, y un PR automático fallaría.

## Fuera del plan

- **Extraer la SDK a `geko-sdk`:** deseable, sin fecha.
- **Enlazar los módulos en un solo binario:** exigiría medir y revisar la regla de que el host no depende de ningún módulo.
- **Hash de código en tiles y router** (P9 del informe): propuesto, independiente de este plan.
