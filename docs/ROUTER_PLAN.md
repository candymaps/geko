# Módulo `geko-router`: plan

Estado: borrador (2026-11-03). Nada implementado. Las decisiones abiertas están al final.

## Objetivo

Un módulo de enrutamiento de `geko`, en Rust, que mejore a OSRM con los mismos objetivos del proyecto: **velocidad, eficiencia y calidad**. Vive en su propio repositorio (`candymaps/geko-router`), se publica como artefacto firmado y se carga como `DylibModule`, según [MODULES_PLAN.md](MODULES_PLAN.md). El host no se toca: rutas bajo `/router/`, configuración en `geko-router.toml`, parte propia del bundle.

Que sea más rápido que OSRM es una **hipótesis**, no un dato. Se da por cierta solo con la medición A/B de la fase 1 (sección «Medición»).

## Reparto

| | Host (`candymaps/geko`) | Módulo (`candymaps/geko-router`) |
|---|---|---|
| Código | Lo de siempre: descarga y carga del módulo, bundles, HTTP común, atribución | Lectura del OSM, grafo, índice, consultas, `/router/v1/*`, caché, órdenes de depuración |
| Datos | Ninguno | Packs de país (`countries/<país>/`), incluidos los de tráfico |
| Contrato | `geko-module-sdk` (sin cambios previstos) | `describe`, `plan`, `fetch`, `build`, `load`, `handle`, `metrics`… |

Regla de siempre: lo que solo tiene sentido para el router (grafo, perfiles, proveedores de tráfico) vive en el módulo. Cualquier cambio que pida tocar el contrato se trata como cambio de contrato (AGENTS.md del host).

## Fuentes de datos

- **Red viaria:** OpenStreetMap, el mismo extracto que ya baja el geocoder (Geofabrik, por ámbito). El módulo declara sus propias descargas con el `Downloader` del host; la caché de descargas evita bajar dos veces el mismo fichero.
- **Tráfico:** en **packs de país**, igual que CartoCiudad en el geocoder (sección siguiente).

## Datos de tráfico en packs por país

Los datos de tráfico dependen del país: cada administración publica en un formato, con una cobertura y una licencia distintas. Por eso, como las fuentes de CartoCiudad, **van en el pack del país** y el motor no sabe de ninguna fuente concreta.

```
countries/es/
├── pack.toml             # país, versión, perfiles por defecto, calendario (festivos), zona horaria
├── data/
│   ├── road-classes.toml # equivalencias de clases de vía para velocidades por defecto
│   ├── calendar.toml     # festivos nacionales y por región (para los perfiles horarios)
│   └── regions.toml      # particiones del grafo (provincias)
├── sources/
│   ├── osm.toml          # extractos de OSM del país (igual que el del geocoder)
│   ├── madrid-sensors.toml   # tráfico en vivo: sensores de Madrid
│   └── dgt-datex2.toml       # incidencias y cortes de la DGT
├── golden/               # rutas de referencia con y sin tráfico
└── tests/
```

Como en el geocoder, el pack es **datos, no código**: lo que cambia entre países es una declaración TOML (`kind`, URL, licencia, formato, frecuencia, cómo se casa el dato con el grafo). El código genérico vive en proveedores por `kind` (`datex2`, `csv-sensors`, `json-sensors`, `ogc-api`…). Añadir un país o una fuente nueva de un tipo ya conocido **no toca el motor**. Un tipo de fuente nuevo es un proveedor nuevo, y no se extrae un trait hasta que haya una segunda implementación real (regla de abstracción selectiva); los dos primeros tipos, sensores y DATEX II, ya lo son.

Cada declaración de fuente lleva, como la de CartoCiudad, `license = { name, attribution, url }`, que entra en el manifiesto del bundle y en `/v1/attribution`. Cada una se puede anular o desactivar desde `geko-router.toml` (`[sources.<nombre>] enabled = false`). Se publican firmados en `ghcr.io/candymaps/geko-router/countries/<país>` con el mecanismo genérico de `geko-artifact`, con etiqueta `pack-<versión>` y la misma regla de inmutabilidad que los del geocoder.

### Tres capas de tráfico

Cada capa es opcional y degrada a la anterior si falta:

1. **Perfil histórico** (fase de generación): velocidad típica por arista, hora y día de la semana, aprendida de las mediciones de los sensores abiertos del país y ligada al calendario del pack. Cubre lo que se haya medido alguna vez; el resto usa la velocidad libre del perfil. Es la capa más barata y la que más aporta por sí sola.
2. **Sensores en vivo** (servicio): cada punto de medida se casa con una o varias aristas del grafo (map matching por proximidad, sentido y clase de vía, hecho una vez al generar) y su dato modifica el peso. Un dato que caduca (TTL por fuente) decae hacia el perfil histórico.
3. **Incidencias y cortes** (servicio): penalizan o cierran aristas, con su vigencia.

Los pesos nuevos se publican por **recustomización** (ver «Algoritmo») y se intercambian con `arc-swap`, sin parar el servicio y sin reiniciar. El refresco corre en una tarea de fondo con su propio calendario por fuente.

### Honradez de la cobertura

No existe un servicio gratuito y libre que dé velocidad por tramo en todas las carreteras de un país. Cada respuesta indica qué parte de la ruta tuvo dato en vivo, histórico o solo velocidad libre (`traffic.coverage`), para que el cliente no confunda «sin retenciones» con «sin datos».

### Fuentes candidatas (por verificar antes de escribir su pack)

Hallazgos de la investigación del 2026-11-03; **todo lo marcado «sin confirmar» debe comprobarse en la ficha de la fuente antes de comprometerse**.

| País | Fuente | Qué da | Licencia | Estado |
|---|---|---|---|---|
| ES | Ayuntamiento de Madrid, datos abiertos | Más de 4.000 puntos de medida, cada 5 min, intensidad y nivel de servicio (0 fluido, 1 lento, 2 retenciones, 3 congestión); no velocidad directa | CC BY 4.0 en el recurso de intensidad | Revisar cada conjunto y el cálculo del nivel (umbrales por punto) |
| ES | DGT, Punto de Acceso Nacional (`nap.dgt.es`) | Paneles y cámaras en DATEX II v3.6/3.7 (1 min); incidencias | Sin confirmar (aparecen licencias Creative Commons, sin variante) | Varios conjuntos marcados obsoletos; no se halló velocidad ni flujo por tramo |
| LU | cita.lu | Eventos en DATEX II (Alert-C) | Sin confirmar | No se halló flujo ni velocidad |
| NL | NDW | Flujo, velocidad, tiempos de viaje, colas | Sin confirmar | Candidato si se añade el país |
| FI | Digitraffic | JSON y DATEX II | Sin confirmar | Candidato |
| EE, LT | Tarktee; servicio de intensidad | Flujo en vivo | Registro gratuito (EE) | Candidatos |

Las fuentes comerciales (Google, TomTom, HERE) y de socio (Waze) no son de uso libre y quedan fuera.

### Licencias (revisión del 2026-11-03)

Revisión de lectura, no asesoramiento jurídico. geko no aloja estos datos: cada instalación los descarga de la fuente, con `accept_licenses = true`, como CartoCiudad.

**Madrid (sensores): usable.**
- La ficha del conjunto `202087-0-trafico-intensidad` dice «Creative Commons Attribution 4.0 International (CC BY 4.0)».
- CC BY 4.0 permite uso comercial y obras derivadas con atribución. La atribución va a `/v1/attribution`: «Ayuntamiento de Madrid, datos abiertos (CC BY 4.0)».
- Pendiente: no pude abrir la página de «Condiciones de uso» del portal (mi URL dio 404), y en 2016 el Ayuntamiento tenía una licencia propia con condiciones por conjunto. Hay que leer esa página y la ficha del conjunto de histórico antes de publicar el pack.
- Cuidado con la mezcla de licencias, igual que con CartoCiudad (CC BY) junto a OSM (ODbL): los pesos de tráfico y el grafo de OSM viajan en el mismo bundle. Conviene fijar qué parte del bundle es obra derivada de OSM (ODbL) y cuál lleva CC BY, y declararlo en el manifiesto.

**DGT (incidencias): usable con reservas.**
- La ficha del NAP dice «Creative Commons Attribution» (enlaza a la Open Definition, sin versión) y «Licence and Free of charge». Sin autenticación.
- Pero las condiciones de uso que enlaza esa ficha, el aviso legal de la DGT, dicen que los contenidos son titularidad de la DGT y tratan como infracción «la reproducción, distribución, comercialización o transformación no autorizadas», salvo uso personal y privado. No menciona reutilización, el Real Decreto 1495/2011 ni Creative Commons. Las dos cosas no encajan.
- Mientras la DGT no lo aclare por escrito, la fuente queda **desactivada por defecto** en el pack (`enabled = false`). Pedir a la DGT (contacto del NAP) que confirme: licencia y versión, uso comercial, obras derivadas, y la atribución exacta.
- Cobertura: red estatal, salvo País Vasco y Cataluña, que publican las suyas (en el NAP: «Incidencias DT-GV» e «Incidencias SCT»). Fuera de la red estatal, sin dato.
- No trae velocidad ni flujo: solo situaciones (cortes de calzada, de carril, obras, retenciones `slowTraffic`).

### Fuentes verificadas en el pack de España

Comprobadas con una petición real el 2026-11-03:

| Fuente | URL | Tamaño | Cabeceras | Cadencia |
|---|---|---|---|---|
| Madrid, sensores | `https://informo.madrid.es/informo/tmadrid/pm.xml` | 2,2 MB | `ETag`, `Last-Modified` | ~5 min |
| DGT, incidencias v3.7 | `https://nap.dgt.es/datex2/v3/dgt/SituationPublication/datex2_v37.xml` | 7 MB (220 KB con gzip) | `Cache-Control: max-age=39`, sin `ETag` | 1 min |

La DGT pide `Accept-Encoding: gzip`, o serían unos 10 GB al día. Madrid publica las coordenadas en ETRS89 UTM 30N (EPSG:25830) con coma decimal.

## Algoritmo

- **CCH (Customizable Contraction Hierarchies).** La topología (orden de nodos, contracción) se calcula una vez al generar; los pesos se aplican en una fase de **customización** que es rápida y se repite cuando cambia el tráfico. Frente a OSRM: su CH obliga a recontraer con cada cambio de pesos, y su MLD admite pesos nuevos pero consulta más lento; su tráfico es experimental y pide recargar el servidor.
- Referencia pública (reimplementación de terceros, no medición propia): recustomización completa en el orden de decenas de ms tras un cambio de arista, con consulta como punto flojo conocido. Esto se **mide con el grafo real** (España), no se asume.
- **Grafo:** con costes por giro y restricciones de giro de OSM desde el principio (modelarlo después obliga a rehacer el grafo). Perfiles en TOML (coche primero; bici, a pie y camión después), sin scripts.
- **Memoria:** ficheros mapeados (`mmap`), como el índice del geocoder, para arranque inmediato y que la caché de páginas del sistema sea compartida. Particiones por provincia/región con el mismo mecanismo de sellos que el resto.
- **Asignador:** mimalloc declarado en la `.so` del módulo (ver MODULES_PLAN.md, «Asignador»).

## API (`/router/v1/…`)

Fase 1: `route` (con alternativas y pasos), `nearest`, `table` (matriz muchos-a-muchos). Después: `match` (map matching), `isochrone`, `trip`. Todas bajo `/router/`, con su `openapi.yaml` y su demo en `/router/`, como el geocoder.

Parámetros de tráfico: `traffic=none|historical|live` y `depart_at` (RFC 3339) para ruta futura con perfil histórico. Por defecto, `live` si hay dato; la respuesta lleva `traffic.coverage`.

Formato: propio y documentado (ver decisiones abiertas).

## Configuración (`geko-router.toml`)

```toml
[packs]
registry = "ghcr.io/candymaps/geko-router/countries"

[profiles]
default = "car"

[traffic]
enabled = true
live = true

[sources.madrid-sensors]    # cualquier clave de countries/es/sources/madrid-sensors.toml
enabled = true

[serve]
countries = []
preload = true
```

Las opciones que cambian lo generado (fuentes, perfiles) entran en los sellos de las etapas. Una opción desconocida es un error que nombra el fichero y la clave, como en el geocoder.

## Bundle y generación

Etapas por país con sellos de entrada (hash de código y de entradas, como PROJECT.md §8), deterministas:

1. Lectura de OSM y construcción del grafo con giros y restricciones.
2. Orden de nodos y contracción (CCH): lo caro, se hace una vez.
3. Casado de los puntos de medida de las fuentes de tráfico del pack con aristas.
4. Perfiles históricos (si hay mediciones descargables).
5. Índice espacial para `nearest` y snapping (R-tree de Hilbert, como la inversa del geocoder).

El tráfico en vivo **no** va en el bundle, que es inmutable: se lee en servicio y vive en memoria.

## Medición

Antes de afirmar «más rápido que OSRM»:

- Mismo extracto OSM, misma máquina, consultas idénticas, 16 peticiones concurrentes, turnos alternos (regla de AGENTS.md: nunca comparar con una medición de otro momento).
- Rutas cortas, medias y largas; `table` de 10×10 a 1.000×1.000; arranque en frío y en caliente; memoria residente; tamaño del bundle; tiempo de generación.
- Con tráfico: tiempo de recustomización y consulta durante la customización.
- Calidad: mismos pares origen-destino contra OSRM y contra una referencia (diferencia de duración y distancia, rutas distintas). La velocidad sin calidad no cuenta.
- Cada intento, funcione o no, se anota en «Optimizaciones» del repositorio del módulo.

## Pasos

Cada paso deja resultados verificables antes del siguiente.

1. **Esqueleto del módulo.** Repositorio, `describe`, `plan`/`fetch` del OSM, `handle` que responde 503, conformidad de la SDK en verde. Publicación firmada.
2. **Grafo y perfil de coche.** Grafo con giros y restricciones, `nearest`, golden de Baleares (`ES-PM`) y Luxemburgo.
3. **CCH y `route`.** Orden, contracción, customización, consulta. Primera medición A/B contra OSRM con Baleares, luego España completa.
4. **`table`** y la segunda medición.
5. **Packs de país**: formato `pack.toml`/`sources/`, publicación firmada, packs `es` y `lu` solo con OSM y perfil (sin tráfico aún).
6. **Tráfico: histórico y en vivo.** Proveedores por `kind` (`csv-sensors`/`json-sensors`), casado de sensores, recustomización en caliente, `traffic.coverage`. Primer pack con tráfico real: la fuente de Madrid.
7. **Incidencias** (DATEX II): cortes y penalizaciones.
8. **`match`, isócronas, bici, a pie, camión.**
9. **Documentación** en cada paso: este plan, PROJECT.md (módulos, §14 hoja de ruta y §16 pendientes), README del host (módulo nuevo) y del módulo.

## Riesgos

- **Cobertura de tráfico escasa y desigual.** Mitigación: tres capas con degradación y `traffic.coverage` explícito.
- **Casar sensores con aristas mal** da retenciones fantasma. Mitigación: casado conservador (sentido, clase de vía, distancia máxima), con test por sensor.
- **Licencias de las fuentes** sin confirmar (DGT, cita.lu): sin licencia clara no se empaqueta.
- **Una fuente que cambia de formato o se da de baja** (varios conjuntos de la DGT figuran como obsoletos): el pack es versionado y una fuente caída degrada, no rompe.
- **Memoria y tiempo de generación** de CCH con España completa: se mide en el paso 3 antes de seguir.
- **Un fallo grave del módulo tira geko** (`DylibModule`, MODULES_PLAN.md, «Riesgos»).

## Decisiones abiertas

- **Alcance de la v1:** solo coche, `route`/`table`, sin tráfico (recomendado: la base medida primero).
- **Formato de la API:** propio, o compatible con OSRM para poder sustituirlo sin tocar clientes (el host eliminó antes los modos Pelias y Photon; ahí se optó por no mantener formatos ajenos).
- **Primer país con tráfico:** Madrid (CC BY 4.0, sensores, el dato más claro) frente a la DGT (cobertura estatal, solo incidencias, licencia por confirmar).
- **Calendario por país:** si los festivos entran en el pack (`calendar.toml`) o se toman de una fuente genérica.
