# Dashboard: plan

Estado: la primera entrega está hecha en `feature/dashboard` (2026-10-26): el shell, el mapa, y una pestaña vacía por módulo activo. Sustituye a la página de demostración (`demo.html`, retirada) por un dashboard que, con el tiempo, sea el panel de control del motor. Queda por hacer lo que dice «Después» y «Por decidir».

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
- La ruta de `/admin/api/` va **detrás de la clave de API**, como cualquier ruta que no sea pública.

### El shell lo sirve el host (`single`) o el operador (`operator`)

El dashboard es plano de control, no de datos. En modo operador las peticiones de usuarios van del Ingress al pod del módulo sin pasar por geko; el shell, que es de administración, no está en ese camino y no lo compite.

### Diseño

Basado en el backoffice de GOIB: un sidebar fijo (272 px abierto, 74 px plegado; `Ctrl + .` lo pliega), una barra superior de 60 px con el icono, el título y, en los módulos, su versión, y el contenido enmarcado, con margen, borde claro y la sombra de las tarjetas. Bajo 1024 px el sidebar flota sobre el contenido; bajo 768 px es un menú lateral. El tema claro y el oscuro siguen al sistema. El color primario es `#A4CC47`, con un verde más oscuro para el texto y los iconos sobre fondo claro.

### El mapa

- MapLibre nativo. Con `tiles` activo y con estilo, dibuja nuestras teselas (estilo claro u oscuro e idioma del navegador); si no, OpenStreetMap.
- El buscador del geocoder (autocompletar, búsqueda, geocodificación inversa, pin de foco y caja visible) solo aparece si `geocoder` está activo.
- **Sin límite al ámbito:** el mapa se mueve y se aleja libremente. Vista inicial: el `center` del `tilejson` de tiles si está activo, o el centro de España.

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
2. **Refrescar al llegar un bundle nuevo.** El dashboard lee la lista de módulos una sola vez; si se reconstruye con otro ámbito, hay que recargar la página. Un sondeo ligero de `/health` (que trae el bundle) lo cerraría.
3. **Lista de módulos en modo operador.** El endpoint lo responde el operador a partir del ConfigMap y del estado de los `Deployment`. Hay que decidirlo junto con las rutas comunes (OPERATOR_MODE_PLAN, «Por decidir», 3).
4. **Compartir Preact** entre shell y paneles (*import map*) o paneles autocontenidos.
5. **Brotli o gzip** para los estáticos incrustados, según la medida de tamaño y el coste de CPU al arrancar.
6. **Limitar el mapa al ámbito.** Se probó (2026-10-26) que cada módulo declarase el área de su parte (`Built::bounds`) y que el host las uniera y las sirviera; se retiró. El host solo tiene el ámbito como códigos ISO, sin geometría, y no conoce las fuentes de cada módulo. Si se retoma, hace falta una fuente común de cajas por región; una sola caja, además, deja huecos con ámbitos separados (Baleares y Canarias).
