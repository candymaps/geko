# Dashboard: plan

Estado: propuesta, nada implementado. Sustituye a la página de demo (`crates/geko-server/src/demo.html`) por un dashboard que, con el tiempo, sea el panel de control del motor.

## Objetivo

- Una aplicación pequeña, que no consuma recursos apreciables ni en el servidor ni en el cliente.
- Una página principal con el mapa y el buscador del geocoder, y una pestaña por cada módulo activo.
- Cada módulo aporta su propia vista, de modo que un cambio en ella se publica con una versión del módulo y no del host.
- Preparado para el [modo operador](OPERATOR_MODE_PLAN.md): los módulos irán en contenedores distintos.

## Decidido

### Preact + Vite + TypeScript, incrustado en el binario

- **Preact** y no React: la misma API (JSX, hooks) por unos 4 KB en vez de unos 45 KB. Si algún día hace falta React, el cambio es casi un alias.
- MapLibre es lo único pesado y ya se cargaba. Con carga perezosa por ruta, el shell sin mapa pesa unas decenas de KB comprimidas.
- Vite compila a estáticos, que se incrustan en el binario precomprimidos (gzip o brotli). Servirlos es copiar bytes, como las teselas. No hay Node en la imagen ni en producción: solo en la compilación, como el perfil de tiles.
- El `dist/` no se versiona; lo genera la CI y `make ui` en local.

### El shell lo sirve el host (`single`) o el operador (`operator`)

El dashboard es plano de control, no de datos. En modo operador las peticiones de usuarios van del Ingress al pod del módulo sin pasar por geko; el shell, que es de administración, no está en ese camino y no lo compite.

### Cada módulo sirve su propia vista

- Por convención, bajo sus rutas: `/<módulo>/ui/panel.js`. El host no la conoce ni la incrusta.
- Así la vista se versiona con el módulo. En modo operador va a su pod, y el Ingress la enruta como el resto de `/<módulo>/*`, en el mismo origen.
- **Sin cambios en el SDK por ahora.** La ruta es una convención fija que el shell sondea; si un módulo no tiene panel, la pestaña muestra un marcador vacío. Si más adelante un módulo necesita declarar algo más (otra ruta, permisos), se evaluará un campo en `Module`, que cambiaría el tag `sdk-<contract>`.

### Contrato del panel

Un módulo ES que exporta:

```ts
export function mount(el: HTMLElement, ctx: PanelContext): void
export function unmount(el: HTMLElement): void
```

con `PanelContext` = `{ base: string, lang: string, apiKey?: string }`. El contrato lleva versión (`panel-contract: 1`), y un cambio incompatible exige coordinar el shell y los módulos, como el del SDK. Cada panel va autocontenido, o comparte Preact mediante un *import map* del shell, para no repetir la biblioteca por módulo.

### Solo lectura

Por ahora el dashboard no cambia nada. Las acciones (reconstruir, recargar bundle, editar configuración) vendrán después y con autenticación (las claves de API de `access.rs`). Cuando llegue la edición de configuración habrá que decidir cómo se reparte entre el ConfigMap, el `BundleStore` y las rutas de administración de cada módulo, porque en modo operador el host no escribe en el disco de otro contenedor.

## Primera entrega

1. **Endpoint** `GET /admin/api/modules`, de solo lectura: los módulos activos, con su nombre y su versión.
2. **Shell** en `ui/`: la página principal y las pestañas.
   - **Principal (`/`)**: el mapa de la demo actual. Con `tiles` activo y con bundle, lo pinta con las teselas del módulo (estilo claro u oscuro, idioma del navegador); si no, con OpenStreetMap. El flyout del buscador del geocoder solo aparece si `geocoder` está activo.
   - **Pestañas**: una por cada módulo que devuelve el endpoint. De momento, vacías: el shell no carga todavía `panel.js`.
3. **Compilación**: `make ui` y su paso en la CI; incrustación en `geko-server` en lugar del `include_str!` actual.
4. **Retirada** de `demo.html` y `docs.html` solo si el shell los sustituye; `/docs` (OpenAPI) se queda.

## Después

- Carga de `panel.js` de cada módulo con el contrato de arriba, empezando por un panel de estado de `tiles` (bundle en uso, tamaño, zooms) y otro del geocoder.
- Métricas (`metrics.rs`) en el dashboard.
- Acciones y edición de configuración, con autenticación.

## Por decidir

1. **Ruta del dashboard.** Si el panel vive en `/admin` y el mapa en `/`, o todo bajo `/` con pestañas.
2. **Lista de módulos en modo operador.** El endpoint lo responde el operador a partir del ConfigMap y del estado de los `Deployment`. Hay que decidirlo junto con las rutas comunes (OPERATOR_MODE_PLAN, «Por decidir», 3).
3. **Compartir Preact** entre shell y paneles (*import map*) o paneles autocontenidos.
4. **Brotli o gzip** para los estáticos incrustados, según la medida de tamaño y el coste de CPU al arrancar.
