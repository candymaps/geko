# Modo operador: plan

Estado: propuesta, nada implementado. Recoge el análisis del 2026-10-24 sobre cómo desplegar geko en Kubernetes sin perder el aislamiento ni el escalado independiente de cada módulo.

## Problema

Hoy los módulos se cargan como bibliotecas en el proceso de geko (`DylibModule`, [MODULES.md](MODULES.md), «Adaptador»). Para un `docker run` es lo ideal, pero en un cluster:

- **Un fallo grave de un módulo tira los demás.** Un pánico solo falla su petición (`catch_unwind`), pero un segfault, un desbordamiento de pila o quedarse sin memoria tiran geko entero ([PROJECT.md](PROJECT.md) §1).
- **No se escalan por separado.** Tiles (disco, red, muchas peticiones pequeñas) y el geocoder (RAM, CPU por consulta) no pueden tener réplicas, `requests` ni `limits` distintos en el mismo pod.
- **No se actualizan por separado.** Cambiar la versión de un módulo exige reiniciar el proceso, y con él los demás.

La escalabilidad horizontal no se rompe (réplicas idénticas tras un balanceador, PROJECT.md §7), pero sí se pierden el aislamiento y el escalado por módulo.

## Qué comparten los módulos

Para saber si el host aporta algo más allá de la comodidad, se miró qué datos comparten tiles y el geocoder (geko-tiles `docs/MODULE_PLAN.md` y las fuentes del geocoder):

| Entrada | Geocoder | Tiles |
|---|---|---|
| Extracto OSM de Geofabrik (`<región>-latest.osm.pbf`) | ✅ | ✅ (mismas URLs, por plan) |
| Overture Places | ✅ | ❌ |
| CartoCiudad (España) | ✅ | ❌ |
| Polígonos de agua y tierra (osmdata), Natural Earth, landcover, qrank | ❌ | ✅ |
| La fuente única fusionada (una entidad por calle o dirección) | ✅ la genera | ❌ no la usa |

- **Solo se comparte la descarga del PBF.** Con Baleares, 47 MB de los 2,35 GB de entradas de tiles (alrededor del 2 %). Cada módulo lo lee con su propio lector (Planetiler en Java, `osmpbf` en Rust): las pasadas de OSM no se comparten.
- **La fuente única fusionada es interna del geocoder.** Tiles no podría usarla: ningún módulo depende de otro (PROJECT.md §2.2).
- **Lo que de verdad comparten es infraestructura:** el `Downloader`, los artefactos firmados, las etapas incrementales, los bundles con recarga en caliente y lo común del servicio HTTP. Ya vive en crates (`geko-module-sdk`, `geko-fetch`, `geko-artifact`).

Por los datos, nada obliga a que los módulos compartan proceso. El valor del host está en la infraestructura común, en el «todo en uno» para despliegues pequeños y en un solo punto de configuración.

Sin comprobar: si la caché `raw/` comparte de verdad el PBF entre módulos (la misma ruta `raw/<fuente>/`; el punto 48 de PROJECT.md propone carpetas por módulo, que lo romperían), y las cifras con España.

## Decidido

### El modo se elige en la configuración (2026-10-24)

```toml
[deploy]
mode = "single"    # single (por defecto) | operator
```

- **`single`**: todos los módulos en un proceso, como hoy. Vale en cualquier sitio: Docker, compose, un servidor o un pod de Kubernetes.
- **`operator`**: geko hace de plano de control en Kubernetes y crea un `Deployment` por módulo, el build como `Job` y `CronJob`, y las rutas.

**No se detecta solo** (por ejemplo, con `KUBERNETES_SERVICE_HOST`):

- Alguien puede querer un solo contenedor dentro de Kubernetes (despliegue pequeño, pruebas, manifiestos fijos).
- Sin RBAC, el modo operador falla. Activarlo por detección acabaría en errores confusos o en un retroceso silencioso.
- Los equipos de plataforma quieren saber qué crea cada cosa en su cluster.

**`single` por defecto:** fuera de Kubernetes el operador no existe; dentro, con un país y poco tráfico, un pod con todo es más simple y gasta menos; y lo seguro por defecto es no pedir permisos en el cluster. El modo operador compensa con varios módulos de tráfico o recursos distintos, o cuando un fallo de uno no puede afectar a otro.

### El host no está en el camino de las peticiones

En modo operador, las peticiones van del Ingress al pod del módulo, sin pasar por geko. Como proceso aparte con un socket Unix (`ProcessModule`) se perdía del 19 al 55 % de caudal (MODULES.md, «Adaptador»); por la red entre pods sería peor.

### Los módulos no saben en qué modo están

El mismo contrato y el mismo `Context` en los dos modos, nunca un `if mode == operator` en un módulo. Toda la diferencia queda en el host.

### `single` usa el mismo camino que el operador

El build publica un bundle y el servicio lo detecta y lo recarga, con la misma abstracción (`BundleStore`): disco local en `single`, PVC o HTTP (`bundles_url`) en el operador. Así se desarrolla en `single` sobre el mismo camino que llega a producción.

## Cómo funciona el modo operador

### Lo que hay en el cluster

1. **El operador**: un `Deployment` de una réplica que ejecuta geko con `[deploy] mode = "operator"`. No atiende peticiones de usuarios.
2. **El ConfigMap `geko`**: `geko.toml` y, si los hay, los `geko-<módulo>.toml`. Es lo único que se configura.
3. **RBAC**: un `ServiceAccount` que puede crear y borrar `Deployment`, `Service`, `Job`, `CronJob`, `PVC` e `Ingress` o `HTTPRoute`, solo en su namespace.
4. **Almacenamiento de bundles**: un `PVC` de lectura y escritura compartida, o un bucket S3 o compatible (punto 43 de PROJECT.md).
5. **Los pods de cada módulo y del build los crea el operador**: no están en el kustomization.

```yaml
# kustomization.yaml
resources:
  - https://github.com/candymaps/geko/deploy/operator?ref=v1.2.0   # RBAC + Deployment del operador
configMapGenerator:
  - name: geko
    files: [geko.toml]
```

### Primer despliegue

6. Se aplica el kustomization (`kubectl apply -k`, ArgoCD o Flux).
7. El operador lee `geko.toml`: módulos activos, ámbito y opciones.
8. **Resuelve versiones**: la imagen de cada módulo, la más nueva de su contrato o la fijada con `version` (ver «Por decidir», 1).
9. **Lanza el build**: un `Job` por módulo (o por módulo y país) que descarga con el `Downloader`, genera su parte del bundle y la publica en el almacenamiento. El Job tiene sus propios `requests` y `limits`: el pico de memoria del build no toca a los pods que sirven. Tiles lanza Planetiler dentro de su Job.
10. **Despliega los módulos**: un `Deployment` y un `Service` por módulo activo, en modo servicio, con `bundles_url` apuntando al almacenamiento.
11. **Readiness**: cada pod responde 503 en `/health` hasta cargar su bundle, y Kubernetes no le manda tráfico hasta entonces.
12. **Rutas**: un `Ingress` o `HTTPRoute` que manda `/geocoder/*` al servicio del geocoder y `/tiles/*` al de tiles. Las rutas comunes, en «Por decidir», 3.

### Servicio

13. Cliente → Ingress → pod del módulo, sin saltos extra.
14. Cada módulo escala por su cuenta: un `HorizontalPodAutoscaler` por `Deployment`, si se configura.
15. Si el geocoder se queda sin memoria, solo se reinicia su pod. Tiles sigue sirviendo.

### Actualización de los datos

16. Un `CronJob` repite el build cada `[data] max_age_days`. Un build a mano es un `Job` con `--refresh`.
17. El build publica un bundle nuevo.
18. Los pods lo detectan en la siguiente recarga (`reload_secs`), bajan solo lo que cambió y hacen el cambio atómico, sin reiniciar. Es lo que ya hace `bundles_url`.

### Cambios de configuración

19. Se edita `geko.toml` en Git y se aplica de nuevo.
20. El operador ve el cambio del ConfigMap y reconcilia:
    - **Módulo activado**: su build y después su `Deployment`, `Service` y ruta.
    - **Módulo desactivado**: borra su `Deployment`, su `Service` y su ruta; sus datos se quedan (como en el punto 48 de PROJECT.md: no se borra por defecto).
    - **País nuevo**: un build nuevo; los pods recogen el bundle en caliente.
    - **Opciones de servicio** (CORS, claves de API…): actualiza el `Deployment`, con despliegue progresivo.

### Actualización de un módulo

21. Sale una versión nueva de un módulo; con `version` sin fijar, el operador la ve en su comprobación periódica.
22. **Primero el build** con la versión nueva (el bundle guarda qué versión lo generó) y, al terminar, **el despliegue** con la imagen nueva.
23. Kubernetes reemplaza los pods poco a poco; los viejos sirven hasta que los nuevos están listos. Los demás módulos no se enteran.

### Fallos

24. **Cae el operador**: los módulos siguen sirviendo; solo se pausan los cambios y los builds.
25. **Falla un build**: los pods siguen con el bundle anterior; el operador lo dice en sus métricas y en su estado.
26. **Cae un pod de un módulo**: lo reinicia Kubernetes, no el operador.

### Configuración solo del modo operador

Réplicas, recursos o almacenamiento por módulo van en su propia sección, que `single` ignora:

```toml
[deploy]
mode = "operator"

[deploy.geocoder]
replicas = 2
memory   = "4Gi"

[deploy.tiles]
replicas = 4
```

Todo lo demás (`[modules]`, `[[scope]]`, `[data]`, los `geko-<módulo>.toml`) es igual en los dos modos: entre desarrollo y producción solo cambia `[deploy]`, por ejemplo con un overlay de kustomize.

## Alternativa más barata: generar los manifiestos

En vez de un operador que aplica y reconcilia, `geko k8s render` (o un chart de Helm con `geko.toml` como valores) escribe una vez los `Deployment`, `Service`, el `CronJob` de build y las rutas, y se guardan en Git.

- Da casi todo: una sola configuración y cada módulo en su contenedor.
- Sin bucle de reconciliación ni permisos en el cluster; todo revisable en Git y aplicado por ArgoCD o Flux.
- Se pierde la automatización de los puntos 20 y 21: cambiar `geko.toml` o subir de versión es volver a generar y aplicar. Se puede llevar a la CI, o a kustomize con un generador KRM o `helmCharts:`, que piden activar plugins en ArgoCD o Flux.
- Los puntos 18, 23 y 26 funcionan igual, porque son de Kubernetes y de `bundles_url`.

Algunos equipos de plataforma no aceptan operadores de terceros con permisos para crear recursos; para ellos, esta es la opción.

## Desarrollo y pruebas

`single` no pone a prueba lo que solo existe en el modo operador:

| En modo operador | En `single` |
|---|---|
| Cada módulo en su proceso, solo con sus bundles | Todos en el mismo proceso |
| Build en un Job; bundles por almacenamiento compartido o `bundles_url` | Build y servicio en el mismo proceso, en disco local |
| Rutas en el Ingress | Rutas en axum, en el host |
| `/v1/attribution` y `/docs` por módulo o agregados | Los sirve el host |
| `/health` por módulo (sonda de readiness) | Un `/health` común |
| RBAC, recursos por pod, orden build → despliegue | No aplica |

Por eso:

1. **A diario, `single`**, que usa el mismo camino de bundles que el operador («Decidido»).
2. **Al tocar algo del host, procesos separados en local**: `geko build`, `geko serve --module geocoder` y `geko serve --module tiles` con `bundles_url`, a mano o en un `docker-compose`. Es el modo operador sin Kubernetes.
3. **En la CI, el modo operador en un cluster** con `kind` o `k3d` y Luxemburgo: aplicar el kustomization, esperar a los pods, consultar `/geocoder/` y `/tiles/`, tirar un pod y comprobar que el otro módulo sigue respondiendo.

## Por decidir

1. **Una imagen o una por módulo.**
   - *Una imagen `geko` con todos los módulos enlazados* al compilar: el operador crea `Deployment` de la misma imagen con `--module <módulo>`. Una sola imagen que publicar y el mismo código en los dos modos, sin descargas de `.so`. A cambio, la imagen es más grande y las versiones de los módulos van con las de geko.
   - *Una imagen por módulo* (`ghcr.io/candymaps/geko-<módulo>`, el host con ese módulo enlazado mediante `geko_app::main(linked)`): cada módulo publica a su ritmo; el operador resuelve la etiqueta por contrato.
2. **Qué pasa con `DylibModule` y la descarga de módulos firmados.** Si todos los módulos son nuestros, la carga dinámica es complejidad sin retorno (ABI, versiones, glibc de la imagen) y podría quitarse. Solo se justifica si queremos módulos de terceros que se enchufen sin recompilar geko.
3. **Las rutas comunes** (`/v1/attribution`, `/docs`, `/metrics`): servidas por cada módulo desde la SDK, o agregadas por un servicio pequeño del operador.
4. **Ingress o Gateway API** (`HTTPRoute`), o los dos.
5. **ConfigMap o CRD.** Un CRD `Geko` da `status` y validación, pero hay que instalarlo y versionarlo; el ConfigMap mantiene `geko.toml` tal cual.
6. **El almacenamiento de bundles**: PVC compartido (no todos los clusters lo tienen) o S3 (punto 43 de PROJECT.md).
7. **Si la caché `raw/` se comparte** entre los Jobs de build de distintos módulos, o cada uno baja lo suyo (afecta al punto 48 de PROJECT.md).

## Pasos

1. **Medir con España** lo que comparten tiles y el geocoder: el tamaño de los extractos de OSM frente a las entradas globales de tiles, y el tiempo de lectura de OSM en cada build.
2. **Decidir los puntos 1 y 2 de «Por decidir»**: de ellos depende todo lo demás.
3. **`[deploy] mode`** en `geko.toml`, con `single` por defecto y `operator` como error claro mientras no exista.
4. **Servicio por módulo**: `geko serve --module <módulo>` y `geko build --module <módulo>`, con su `/health` y sus rutas comunes (punto 3 de «Por decidir»).
5. **`single` sobre el camino de bundles del operador**, con `BundleStore`.
6. **Procesos separados en local** (`docker-compose` de ejemplo) para desarrollar el host.
7. **`geko k8s render`** o un chart de Helm, con su prueba en `kind` o `k3d` en la CI.
8. **`BundleStore` con S3** (punto 43 de PROJECT.md), si el PVC compartido no basta.
9. **El operador** (`kube-rs`), cuando haya alguien que lo pida, con la misma prueba en la CI que el paso 7.

## Coste del operador

Es el paso caro y el que más mantenimiento arrastra:

- Código nuevo: la lectura del ConfigMap (o el CRD), el bucle de reconciliación, el RBAC, el orden build → despliegue, el estado y la limpieza.
- Mantenimiento: versiones de la API de Kubernetes, distintos controladores de Ingress, las políticas de cada cluster.
- Aceptación: algunos equipos no instalan operadores de terceros.

Por eso va al final, después de la alternativa de generar los manifiestos.
