# Dashboard: plan

Estado: la primera entrega está hecha en `feature/dashboard` (2026-10-26): el shell, el mapa, una pestaña vacía por módulo activo y el ámbito (`scope`) que el host sirve. Sustituye a la página de demostración (`demo.html`, retirada) por un dashboard que, con el tiempo, sea el panel de control del motor. Queda por hacer lo que dice «Después» y «Por decidir».

## Objetivo

- Una aplicación pequeña, que no consuma recursos apreciables ni en el servidor ni en el cliente.
- Una página principal con el mapa y el buscador del geocoder, y una pestaña por cada módulo activo.
- Cada módulo aporta su propia vista, de modo que un cambio en ella se publica con una versión del módulo y no del host.
- Preparado para el [modo operador](OPERATOR_MODE_PLAN.md): los módulos irán en contenedores distintos.

## Decidido y hecho

### Preact + Vite + TypeScript, incrustado en el binario

- **Preact** y no React: la misma API (JSX, hooks) por unos 4 KB en vez de unos 45 KB. Si algún día hace falta React, el cambio es casi un alias. Los iconos son `lucide-preact`.
- **Medidas** (2026-10-26): el shell pesa unos 10,5 KB comprimidos; MapLibre, que va en un trozo aparte de carga perezosa, unos 288 KB comprimidos. En el binario, `ui/dist` ocupa unos 1,4 MB en crudo.
- **Compilación:** `ci/ui.sh` tipa y compila `ui/` en Docker (`node:22-slim`, el Node de su `package-lock.json`) sobre una copia, y deja `ui/dist`, que no se versiona. `make ui` lo hace solo si cambian las fuentes; `make bin` e `image` dependen de él. El `Dockerfile` tiene una etapa `ui` para la imagen publicada y la de `make start-dev`. No hay Node en la imagen ni en producción.
- **Incrustación:** `crates/geko-server/build.rs` mete cada fichero de `ui/dist`, en crudo y en gzip (comprimido al compilar, determinista), y el servidor lo responde copiando bytes, en gzip solo si el cliente lo acepta. Los ficheros de `/assets/` llevan hash y se cachean para siempre; la página, con `no-cache`. Sin `ui/dist` el Rust compila igual, y `/` dice que se compile con `make ui`. Se probó solo gzip; brotli queda por medir.
- **CI:** variante `ui` de `ci/test.sh` (corre la primera, para que lo demás compile con el dashboard dentro); `ci/affected.sh` distingue los cambios solo de `ui/` de los de Rust.

### Rutas

- El shell se sirve en `/`, sin clave de API, con sus ficheros. La navegación usa el hash (`#/tiles`), así que no hacen falta rutas de reserva en el servidor para los enlaces directos.
- `GET /admin/api/modules`: los módulos activos, con su nombre, su versión viva y si no están disponibles.
- `GET /admin/api/scope`: el área que cubren los módulos activos, `{"bounds": [oeste, sur, este, norte] | null}`.
- Ambas rutas de `/admin/api/` van **detrás de la clave de API**, como cualquier ruta que no sea pública.

### El shell lo sirve el host (`single`) o el operador (`operator`)

El dashboard es plano de control, no de datos. En modo operador las peticiones de usuarios van del Ingress al pod del módulo sin pasar por geko; el shell, que es de administración, no está en ese camino y no lo compite.

### Diseño

Basado en el backoffice de GOIB: un sidebar fijo (272 px abierto, 74 px plegado; `Ctrl + .` lo pliega), una barra superior de 60 px con el icono, el título y, en los módulos, su versión, y el contenido enmarcado, con margen, borde claro y la sombra de las tarjetas. Bajo 1024 px el sidebar flota sobre el contenido; bajo 768 px es un menú lateral. El tema claro y el oscuro siguen al sistema. El color primario es `#A4CC47`, con un verde más oscuro para el texto y los iconos sobre fondo claro.

### El mapa

- MapLibre nativo. Con `tiles` activo y con estilo, dibuja nuestras teselas (estilo claro u oscuro e idioma del navegador); si no, OpenStreetMap.
- El buscador del geocoder (autocompletar, búsqueda, geocodificación inversa, pin de foco y caja visible) solo aparece si `geocoder` está activo.
- **Limitado al ámbito:** el mapa no se desplaza ni se aleja más allá de la caja del ámbito, con un margen (`MARGIN`, 25 %), de modo que no se ve «vacío» con las teselas vectoriales, que solo cubren el ámbito. Aplica también cuando las dibuja OpenStreetMap.

### El ámbito lo declaran los módulos y lo une el host

El host solo tiene el ámbito como códigos ISO, sin geometría, y no debe conocer las fuentes de cada módulo. Por eso cada módulo dice qué cubre su parte y el host las une:

- **SDK:** `Built::bounds` y `BundleModule::bounds`, opcionales (`[oeste, sur, este, norte]` en grados). Es aditivo: un bundle anterior o un módulo que no lo sabe no declara ninguno, y el contrato no cambia. Cruza el adaptador de bibliotecas como JSON, donde un campo que falta es `None`.
- **Host:** al cargar un bundle une las áreas de los módulos activos (`BundleManager::bounds`) y las sirve en `/admin/api/scope`. Un módulo apagado no cuenta.
- **tiles:** declara la unión de las cajas del ámbito, que salen de las cabeceras de los extractos de OSM.
- **geocoder:** el índice inverso guarda en su manifiesto la extensión de sus puntos y el build une las de los países. Es más ceñida que la de tiles (Baleares: 1,16 / 38,64 / 4,33 / 40,09, frente a 0,62 / 38,06 / 4,94 / 40,71) y no depende de que se hayan borrado las descargas.
- **Dashboard:** usa el `bounds` del host; si no hay (un bundle anterior), el del `tilejson` de tiles; si tampoco, el centro de España como vista por defecto, la única constante del ámbito en la interfaz.
- **Rendimiento:** solo toca metadatos. Servir teselas o buscar no cambia; el coste está en el build (leer una columna de extensión) y en el manifiesto (cuatro números por módulo).
- **Efectos:** el manifiesto del índice inverso del geocoder cambia, así que su golden pedirá actualizar la línea base. Los módulos deben compilarse contra un SDK que tenga el campo, y los literales de `BundleModule` de sus tests y ejemplos piden `bounds: None`.

### Cada módulo sirve su propia vista

- Por convención, bajo sus rutas: `/<módulo>/ui/panel.js`. El host no la conoce ni la incrusta.
- Así la vista se versiona con el módulo. En modo operador va a su pod, y el Ingress la enruta como el resto de `/<módulo>/*`, en el mismo origen.
- **Sin cambios en el SDK por ahora.** La ruta es una convención fija que el shell sondeará. Hoy el shell **no carga** ningún `panel.js`: la pestaña de cada módulo está vacía.

### Contrato del panel (por implementar)

Un módulo ES que exporta:

```ts
export function mount(el: HTMLElement, ctx: PanelContext): void
export function unmount(el: HTMLElement): void
```

con `PanelContext` = `{ base: string, lang: string, apiKey?: string }`. El contrato lleva versión (`panel-contract: 1`), y un cambio incompatible exige coordinar el shell y los módulos, como el del SDK. Cada panel va autocontenido, o comparte Preact mediante un *import map* del shell.

### Solo lectura

Por ahora el dashboard no cambia nada. Las acciones (reconstruir, recargar bundle, editar configuración) vendrán después y con autenticación. Cuando llegue la edición de configuración habrá que decidir cómo se reparte entre el ConfigMap, el `BundleStore` y las rutas de administración de cada módulo, porque en modo operador el host no escribe en el disco de otro contenedor.

## Después

- Cargar `panel.js` de cada módulo con el contrato de arriba, empezando por un panel de estado de `tiles` (bundle en uso, tamaño, zooms) y otro del geocoder. Un `ui/` en cada repositorio de módulo pedirá un paso en su `ci/modules.sh`, como `ci/profile.sh` ya usa Node en Docker.
- Métricas (`metrics.rs`) en el dashboard.
- Acciones y edición de configuración, con autenticación.

## Por decidir

1. **Claves de API.** Con claves activas, `/` y sus ficheros cargan sin clave, pero el dashboard llama a `/admin/api/*` y a las rutas de los módulos, y recibirían 401. Hace falta una forma de que el dashboard tenga la clave (un campo que la pida y la guarde, o una cabecera) sin dejarla en la URL.
2. **Refrescar al llegar un bundle nuevo.** El dashboard lee la lista de módulos y el ámbito una sola vez; si se reconstruye con otro ámbito, hay que recargar la página. Un sondeo ligero de `/health` (que trae el bundle) lo cerraría.
3. **Mover el tag del SDK.** Los módulos dependen de `sdk-<contrato>`; el campo nuevo solo existe en el árbol del host hasta que llegue a `dev` y el tag avance. Hasta entonces sus ramas `feature/declare-bounds` no compilan en su CI.
4. **Lista de módulos en modo operador.** El endpoint lo responde el operador a partir del ConfigMap y del estado de los `Deployment`. Hay que decidirlo junto con las rutas comunes (OPERATOR_MODE_PLAN, «Por decidir», 3).
5. **Compartir Preact** entre shell y paneles (*import map*) o paneles autocontenidos.
6. **Brotli o gzip** para los estáticos incrustados, según la medida de tamaño y el coste de CPU al arrancar.
7. **Un rectángulo único para el ámbito.** La unión de cajas de un ámbito con varias zonas separadas (Baleares y Canarias) deja huecos sin datos dentro de la caja. Es una limitación de usar una sola caja.
