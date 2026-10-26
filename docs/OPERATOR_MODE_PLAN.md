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

- **Solo se comparte la descarga del PBF.** Medido el 2026-10-26: con Baleares son 47 MB de unos 2,1 GB de entradas globales de tiles (alrededor del 2 %); con España y Canarias, 1,56 GB frente a esos 2,1 GB (alrededor del 40 %). La cifra anterior de 2,35 GB era previa a bajar Natural Earth por capas (de 446 a 72 MB). Cada módulo lo lee con su propio lector (Planetiler en Java, `osmpbf` en Rust): las pasadas de OSM no se comparten, y con España son una parte importante del build que aún no está medida por módulo.
- **La fuente única fusionada es interna del geocoder.** Tiles no podría usarla: ningún módulo depende de otro (PROJECT.md §2.2).
- **Lo que de verdad comparten es infraestructura:** el `Downloader`, los artefactos firmados, las etapas incrementales, los bundles con recarga en caliente y lo común del servicio HTTP. Ya vive en crates (`geko-module-sdk`, `geko-fetch`, `geko-artifact`).

Por los datos, nada obliga a que los módulos compartan proceso. El valor del host está en la infraestructura común, en el «todo en uno» para despliegues pequeños y en un solo punto de configuración.

### La caché `raw/` ya comparte el PBF (medido el 2026-10-26)

Los dos módulos guardan el extracto en la misma ruta, `raw/osm/<id>-latest.osm.pbf`, con la misma URL de Geofabrik y el mismo `.md5`. En el volumen de desarrollo hay una sola copia y una sola ficha `.raw.json`. El `Downloader` decide reutilizar por la ruta, la ficha (misma URL y mismo tamaño) y un GET condicional; el checksum solo se comprueba al terminar una descarga. Pero compartirlo con seguridad exige resolver esto:

- **`keep_downloads = false` es el valor por defecto.** Tras publicar, cada módulo borra el PBF y deja la ficha. El otro módulo ve la ficha sin fichero y lo vuelve a bajar. En un `raw/` común solo se comparte con `keep_downloads = true` o si el borrado lo decide el host.
- **Descargas a la vez sobre el mismo destino se pisan** (`<destino>.part`): dos Jobs de build que comparten `raw/` hay que serializarlos o darles un bloqueo.
- **`geko build --purge` del geocoder borra lo de tiles** en `raw/` (`osmdata`, `natural-earth`, `planetiler`…) y los PBF fuera de su ámbito, porque solo conserva las carpetas de sus fuentes. Es un fallo que hay que corregir, o un requisito, si se comparte.
- **Cada módulo decide su política de refresco.** En la corrida de Baleares, el geocoder construyó con el extracto del 22-10 y tiles recibió uno nuevo que sobrescribió el fichero: dos módulos con instantáneas distintas de OSM. Habría que decidir si el host impone una política común.
- **El punto 48 de PROJECT.md** (carpetas por módulo) rompería la compartición.

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

### Los bundles van en un PVC compartido, de momento (2026-10-26)

El almacenamiento de bundles del modo operador es un `PVC` con acceso `ReadWriteMany`: el Job de build escribe en él y los pods de cada módulo lo montan para leer y recargar, aunque estén en nodos distintos. Un `ReadWriteOnce` no vale: solo se monta en un nodo a la vez.

- **Por qué un PVC:** es lo que menos añade. Sin cliente de objetos ni credenciales, y el `BundleStore` es el de disco local con otra ruta.
- **Lo que exige:** un cluster con una clase de almacenamiento `ReadWriteMany` (NFS, CephFS, Azure Files, EFS, Filestore…). No todos la tienen.
- **S3 queda para después**, como segundo `BundleStore` (paso 8), si un cluster no ofrece `ReadWriteMany` o el PVC no basta. El operador no lo necesita para empezar.

### Una imagen por módulo (2026-10-26)

El operador despliega, para cada módulo, la imagen `ghcr.io/candymaps/geko-<módulo>`: el host con ese módulo enlazado mediante `geko_app::main(linked)`, como ya hace `crates/geko-host` del geocoder para sus checks. El operador resuelve la etiqueta por contrato (la más nueva del contrato de geko, o la fijada con `version`).

- **Por qué:** cada módulo vive en su repositorio y se publica a su ritmo; una imagen única obligaría a juntar los repositorios en cada release. Así también se cumple la actualización por separado y cada pod solo lleva el código de su módulo.
- **Coste asumido:** la versión del host va dentro de cada imagen, así que un cambio del host exige reconstruir las de cada módulo. El contrato (`sdk-<contrato>`) ya obliga a coordinarlos.
- **Descartada:** una imagen `geko` con todos los módulos enlazados, con `--module <módulo>` en cada `Deployment`. Una sola imagen que publicar, pero más grande, con las versiones de los módulos atadas a las del host.

### `DylibModule` y la descarga de módulos firmados se mantienen (2026-10-26)

No se quitan. Con una imagen por módulo, el modo `operator` ya no los usa, pero `single` sí: es la forma de tener en un solo `docker run` módulos que se publican por separado. Quitarlos obligaría a `single` a usar una imagen combinada, justo lo descartado arriba. Funciona y está medido (sin coste frente a enlazado). Se puede reabrir cuando el operador exista y se vea cuánto se usan.

### Las rutas comunes las sirve cada módulo (2026-10-26)

`/v1/attribution`, `/docs` y `/metrics` los sirve cada módulo, con el código en la SDK; no hay servicio agregador.

- El Ingress ya manda `/<módulo>/*` al pod que toca, así que `/geocoder/v1/attribution` y `/tiles/v1/attribution` no piden nada nuevo en el camino de las peticiones.
- `/metrics` por pod es lo habitual en Kubernetes: Prometheus lo recoge de cada pod, no a través del Ingress.
- En `single` lo sirve el host con el mismo código de la SDK, así que el resultado es idéntico en los dos modos.
- Una página de `/docs` unificada puede generarse más adelante al renderizar los manifiestos.

### `single` usa el mismo camino que el operador

El build publica un bundle y el servicio lo detecta y lo recarga, con la misma abstracción (`BundleStore`): disco local en `single`, PVC o HTTP (`bundles_url`) en el operador. Así se desarrolla en `single` sobre el mismo camino que llega a producción.

## Cómo funciona el modo operador

### Lo que hay en el cluster

1. **El operador**: un `Deployment` de una réplica que ejecuta geko con `[deploy] mode = "operator"`. No atiende peticiones de usuarios.
2. **El ConfigMap `geko`**: `geko.toml` y, si los hay, los `geko-<módulo>.toml`. Es lo único que se configura.
3. **RBAC**: un `ServiceAccount` que puede crear y borrar `Deployment`, `Service`, `Job`, `CronJob`, `PVC` e `Ingress` o `HTTPRoute`, solo en su namespace.
4. **Almacenamiento de bundles**: un `PVC` `ReadWriteMany` (ver «Decidido»). Un bucket S3 o compatible (punto 43 de PROJECT.md) queda para más adelante.
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
8. **Resuelve versiones**: la imagen de cada módulo, la más nueva de su contrato o la fijada con `version` (ver «Decidido», una imagen por módulo).
9. **Lanza el build**: un `Job` por módulo (o por módulo y país) que descarga con el `Downloader`, genera su parte del bundle y la publica en el almacenamiento. El Job tiene sus propios `requests` y `limits`: el pico de memoria del build no toca a los pods que sirven. Tiles lanza Planetiler dentro de su Job.
10. **Despliega los módulos**: un `Deployment` y un `Service` por módulo activo, en modo servicio, con `bundles_url` apuntando al almacenamiento.
11. **Readiness**: cada pod responde 503 en `/health` hasta cargar su bundle, y Kubernetes no le manda tráfico hasta entonces.
12. **Rutas**: un `Ingress` o `HTTPRoute` que manda `/geocoder/*` al servicio del geocoder y `/tiles/*` al de tiles. Las rutas comunes las sirve cada módulo (ver «Decidido»).

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

### Los mismos YAML en local, en la CI y en producción

Para que lo que se sube y lo que se prueba sea idéntico, los manifiestos tienen **una sola fuente**:

- `deploy/` con una base de kustomize (un `Deployment` y un `Service` por módulo, el `CronJob` de build, el PVC y las rutas) y overlays `local`, `ci` y `prod`. Entre overlays solo cambia lo inevitable (réplicas, dominio, clase de almacenamiento) y `[deploy]` de `geko.toml`.
- Si se usa `geko k8s render` (paso 7), su salida es esa base, y el operador (paso 9) reconcilia lo mismo.
- **Sin cluster, en cada PR:** `kustomize build` de cada overlay validado con `kubeconform`, y un diff contra los YAML versionados que falla si se cambia el código sin regenerarlos.
- **Con cluster, en la CI y en local:** crear `kind` o `k3d`, cargar las imágenes del árbol de trabajo (`kind load docker-image` o `k3d image import`, sin registro), `kubectl apply -k deploy/overlays/<overlay>`, esperar a los pods y consultar `/geocoder/` y `/tiles/`. Comprobaciones: tirar el pod de un módulo y ver que el otro sigue respondiendo; subir las réplicas de `geocoder` a 3 y ver que `tiles` se queda en 1.

Lo que un cluster local no reproduce:

- **El PVC `ReadWriteMany`.** `kind` y `k3d` traen almacenamiento `ReadWriteOnce`, que en un cluster de un solo nodo funciona pero no demuestra que el RWX de producción lo haga. Hace falta un aprovisionador NFS local o una prueba aparte en un cluster real.
- **Latencia y límites de memoria reales**, el Ingress de la plataforma y un RBAC endurecido.

### Los `make` de desarrollo siguen el modo

Los comandos de desarrollo no se complican ni se duplican: leen `[deploy] mode` de la misma configuración que ya leen para saber qué módulos construir (`DEV_CONFIG`, hoy en `DEV_MODULES`), y cambian de camino:

| Comando | `single` (por defecto) | `operator` |
|---|---|---|
| `make start-dev` | Como hoy: un contenedor con geko y los módulos (`develop/compose.yaml`) | Crea el cluster local si no existe, construye y carga las imágenes, aplica el overlay `local`, espera a los pods y deja el dashboard en el mismo `http://localhost:8080/` con un `port-forward` |
| `make stop-dev` | Baja el contenedor, conserva su volumen | Borra los recursos del overlay y conserva el cluster y el PVC (o `make stop-dev PURGE=1` lo borra todo) |
| el seguimiento del log | `docker compose logs -f` | `kubectl logs -f` de los pods de los módulos |

- **Misma experiencia:** el mismo comando, la misma URL y la misma configuración. Solo cambia `[deploy] mode`.
- **Idempotente:** si el cluster o las imágenes ya están, se reaprovechan; un cambio de código solo reconstruye la imagen y reinicia los pods.
- **Sin sorpresas:** si falta `kind` o `k3d`, o `kubectl`, `make start-dev` dice qué instalar y no hace nada a medias. Sin `[deploy]`, es `single`.
- **Hasta el paso 9** no hay operador: el modo `operator` de desarrollo aplica los manifiestos del paso 7, que son los mismos que reconciliaría el operador. Cuando exista, el overlay `local` lo desplegará dentro del cluster.
- **Primera ejecución más lenta** (crear el cluster y construir las imágenes); las siguientes, no.
- **Rutas con espacios** en el `Makefile`: hoy no se entrecomilla `DEV_CONFIG` y falla con rutas como `03 Candymaps`. Hay que arreglarlo al tocarlo.

## Por decidir

1. **Ingress o Gateway API** (`HTTPRoute`), o los dos.
2. **ConfigMap o CRD.** Un CRD `Geko` da `status` y validación, pero hay que instalarlo y versionarlo; el ConfigMap mantiene `geko.toml` tal cual.
3. **Si la caché `raw/` se comparte** entre los Jobs de build de distintos módulos, o cada uno baja lo suyo (afecta al punto 48 de PROJECT.md). Compartirla pide un `raw/` común en el PVC con `keep_downloads = true` (o el borrado en el host), serializar las descargas, arreglar el `--purge` del geocoder y una política de refresco común. Si no, cada Job baja lo suyo: con España son 1,56 GB más por módulo.

## Pasos

1. **Medir con España** lo que comparten tiles y el geocoder. Hecho (2026-10-26): los tamaños de los extractos frente a las entradas globales de tiles, y que `raw/` ya comparte el PBF, con condiciones (ver «Qué comparten los módulos»). Pendiente: el tiempo de lectura de OSM en cada build con España. No hay medida que lo separe del resto, y hacerla pide una ejecución con España de cada módulo (30 a 45 min en total, 20 a 40 GB de disco y unos 4-5 GB de RAM de pico, de una en una).
2. ~~**Decidir los puntos 1 y 2 de «Por decidir»**~~ Hecho (2026-10-26): una imagen por módulo, `DylibModule` se mantiene y las rutas comunes las sirve cada módulo (ver «Decidido»).
3. **`[deploy] mode`** en `geko.toml`, con `single` por defecto y `operator` como error claro mientras no exista.
4. **Servicio por módulo**: `geko serve --module <módulo>` y `geko build --module <módulo>`, con su `/health` y sus rutas comunes (ver «Decidido»).
5. **`single` sobre el camino de bundles del operador**, con `BundleStore`.
6. **Procesos separados en local** (`docker-compose` de ejemplo) para desarrollar el host.
7. **`geko k8s render`** o un chart de Helm, con su prueba en `kind` o `k3d` en la CI.
7 bis. **`make start-dev` y `make stop-dev` según `[deploy] mode`**: `single` como hoy; `operator` crea el cluster local y aplica el overlay `local` de `deploy/` (ver «Los `make` de desarrollo siguen el modo»).
8. **`BundleStore` con S3** (punto 43 de PROJECT.md), solo si un cluster no ofrece `ReadWriteMany` o el PVC no basta.
9. **El operador** (`kube-rs`), cuando haya alguien que lo pida, con la misma prueba en la CI que el paso 7.

## Coste del operador

Es el paso caro y el que más mantenimiento arrastra:

- Código nuevo: la lectura del ConfigMap (o el CRD), el bucle de reconciliación, el RBAC, el orden build → despliegue, el estado y la limpieza.
- Mantenimiento: versiones de la API de Kubernetes, distintos controladores de Ingress, las políticas de cada cluster.
- Aceptación: algunos equipos no instalan operadores de terceros.

Por eso va al final, después de la alternativa de generar los manifiestos.
