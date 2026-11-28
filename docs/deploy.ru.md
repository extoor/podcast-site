[English](deploy.md) | **Русский**

# Развёртывание в Kubernetes

Всё разворачивает Helm-чарт `deploy/helm/podcast-site`:

- домен пишется один раз — `domain` (и при желании `extraDomains`); из него берутся
  `Certificate` cert-manager, оба `IngressRoute` Traefik (`web`, порт 80, перенаправляет на
  HTTPS через middleware `redirect-scheme`; у `websecure` middleware нет) и базовый адрес
  в плейлистах;
- `generator` — CronJob (`generator.schedule`) и Job после каждой установки/обновления;
  собранный сайт лежит на PVC ReadWriteOnce (`site.storageClass`, по умолчанию
  `openebs-hostpath`), поэтому чарт держит все поды релиза на одной ноде (`podAffinity`).
  С локальным классом сайт лежит, пока лежит эта нода; с реплицируемым (OpenEBS Mayastor,
  например `mayastor-thin-r3`, Ceph RBD) поды вместе переезжают на другую. Gopher'у на этой
  ноде нужен его сетевой интерфейс — ограничьте ноды через `nodeSelector`;
- аудио — PV rclone CSI-драйвера только на чтение
  ([veloxpack/csi-driver-rclone](https://github.com/veloxpack/csi-driver-rclone),
  `crypt:music/Podcasts`); его конфигурация (ключ `configData`, весь `rclone.conf`) —
  Secret, создаваемый вне чарта (`media.csi.nodePublishSecretRef`), либо свой claim через
  `media.type: existingClaim`. `helm install`/`upgrade` останавливается, если Secret нет или в
  нём нет ключа `configData`. На диск ноды ничего не кэшируется: чарт задаёт
  `vfs-cache-mode=off`, в памяти остаются только списки каталогов и буферы чтения;
- `gopherd` получает свой адрес через Multus (`gopher.network`) и слушает порт 70
  без root (sysctl пода `net.ipv4.ip_unprivileged_port_start=0`); меню ссылаются на
  `gopher.host` (по умолчанию `domain` — то же имя, что у веба);
- все поды проходят Pod Security «restricted»; `networkPolicy.enabled` запрещает исходящие
  соединения и пускает к веб-поду только Traefik.

## Что нужно в кластере

- Traefik с CRD (`traefik.io/v1alpha1`), точками входа `web` и `websecure` и middleware
  `redirect-scheme` в неймспейсе `traefik` (`ingressRoute.*` в values);
- cert-manager и издатель сертификатов (`certificate.issuerRef`, по умолчанию ClusterIssuer
  `zerossl-production`);
- rclone CSI-драйвер [veloxpack/csi-driver-rclone](https://github.com/veloxpack/csi-driver-rclone);
- StorageClass для собранного сайта (`site.storageClass`, по умолчанию `openebs-hostpath`);
- для Gopher — Multus с CNI-плагинами macvlan и static (NetworkAttachmentDefinition создаёт
  чарт), или `gopher.enabled: false`.

## 1. Свои значения — вне git

Репозиторий публичный: настоящий домен и внутренние адреса кладутся в `deploy/local/`,
он в `.gitignore`. Например, `deploy/local/values.yaml`:

```yaml
domain: podcasts.example.org
gopher:
  network:                 # the pod's own address (Multus macvlan)
    enabled: true
    master: eth0.100       # интерфейс ноды (здесь VLAN 100 на eth0)
    address: 192.0.2.41/24
    gateway: 192.0.2.1
web:
  trustedProxies: [<traefik-pod-cidr>]   # по умолчанию private_ranges
```

Все параметры с комментариями — в `deploy/helm/podcast-site/values.yaml`.

## 2. Secret для rclone

Драйвер читает только ключ `configData` — весь `rclone.conf` с секцией `crypt` и remote под
ней (только эти две, без остальных ваших remote). Если конфигурация защищена паролем,
выгрузите незашифрованную копию через `rclone config show`.

```sh
kubectl create namespace podcasts
kubectl -n podcasts create secret generic podcast-site-rclone --from-file=configData=rclone.conf
```

## 3. Установка

```sh
helm upgrade --install podcast-site deploy/helm/podcast-site -n podcasts -f deploy/local/values.yaml
kubectl -n podcasts get pods -w
```

Сразу после установки Job (`podcast-site-generator-init`) собирает сайт; поды веба и Gopher
становятся готовыми, когда он закончит. Проверка:

```sh
kubectl -n podcasts get pods,certificate,ingressroute
kubectl -n podcasts exec deploy/podcast-site-gopherd -- /usr/local/bin/gopherd --check 127.0.0.1:70
kubectl -n podcasts logs deploy/podcast-site-web | tail -3
```

## Сеть

- DNS-имя `domain` указывает на публичный адрес; порты 80 и 443 ведут на Traefik (проверке
  HTTP-01 у cert-manager нужен порт 80).
- Порт 70 публичного адреса переадресуется на собственный адрес пода Gopher. Ответы должны
  уходить через тот же интерфейс, иначе соединение не установится. С `gopher.network.gateway`
  чарт ставит через шлюз маршруты `0.0.0.0/1` и `128.0.0.0/1`: они точнее маршрута по умолчанию
  на `eth0`, который `default-route` Multus заменяет не всегда (с k3s/flannel — нет). Сеть
  кластера остаётся на `eth0`. SNAT на маршрутизаторе тоже решает задачу. Заглянуть в
  distroless-под: `kubectl debug -it <под> --image=nicolaka/netshoot --profile=netadmin -- ip route`.
- Адрес посетителя попадает в лог доступа (первое поле), только если его видит Traefik
  (`externalTrafficPolicy: Local` у его Service, без SNAT перед ним), а поды Traefik входят в
  `web.trustedProxies`.

## Обновление

Образы — `latest` с `pullPolicy: Always`, пересобираются из `main` и раз в неделю. Подтянуть их
и сразу пересобрать сайт (иначе он пересоберётся по `generator.schedule`):

```sh
helm upgrade --install podcast-site deploy/helm/podcast-site -n podcasts -f deploy/local/values.yaml
kubectl -n podcasts rollout restart deploy/podcast-site-web deploy/podcast-site-gopherd
kubectl -n podcasts create job --from=cronjob/podcast-site-generator podcast-site-build-now
```

С ArgoCD первая команда — `argocd app sync podcast-site` (или дождитесь автосинхронизации).
Сам Argo CD нового образа не замечает: тег остаётся `latest`, манифесты не меняются. С Image
Updater (ниже) новые образы выкатываются сами.

## ArgoCD

`deploy/argocd/application.yaml` — пример; свою копию с настоящими значениями держите в
`deploy/local/` (те же значения, что в `deploy/local/values.yaml`). Приложение следит за `main`:
сначала изменения должны попасть туда. Если раньше ставили Helm'ом — удалите только запись о
релизе Helm, ArgoCD подхватит работающие объекты. `helm uninstall` не делайте: он удалит PVC с
собранным сайтом.

```sh
kubectl -n podcasts delete secret -l owner=helm,name=podcast-site
```

Потом создайте приложение — через `kubectl`:

```sh
kubectl apply -f deploy/local/argocd-application.yaml
kubectl -n argocd get application podcast-site -w
```

или через CLI `argocd`:

```sh
argocd login <адрес-argocd>
argocd app create -f deploy/local/argocd-application.yaml
argocd app wait podcast-site --health --sync --timeout 600
argocd app get podcast-site
```

В файле включена автосинхронизация — ArgoCD сразу всё применит. Чтобы сначала посмотреть
diff, создайте приложение с `--sync-policy none` (флаги перекрывают файл):

```sh
argocd app create -f deploy/local/argocd-application.yaml --sync-policy none
argocd app diff podcast-site
argocd app sync podcast-site
argocd app set podcast-site --sync-policy automated --self-heal --auto-prune
```

В работе:

```sh
argocd app get podcast-site --refresh
argocd app diff podcast-site
argocd app sync podcast-site
argocd app history podcast-site
```

- Новый коммит в `main` ArgoCD замечает примерно за 3 минуты; `--refresh` — сразу.
- Job `generator-init` — хук фазы Sync (не PostSync: тот ждал бы подов, которые здоровы только
  с собранным сайтом): каждая синхронизация пересобирает сайт.
- Проверка Secret (`templates/checks.yaml`) требует живого кластера и в ArgoCD пропускается.
- Поды перекатываются сами при смене монтирования аудио (`checksum/storage`), но не при
  пересборке каталога.
- При включённом `selfHeal` ручные правки и `argocd app rollback` возвращаются к тому, что в `main`.

`deploy/kustomize/` — тот же чарт через kustomize (`kustomize build --enable-helm`) для своих
патчей.

## Новые образы без ручных перезапусков: Argo CD Image Updater

[Argo CD Image Updater](https://github.com/argoproj-labs/argocd-image-updater) (v1.x)
следит за тегом `latest` трёх образов в GHCR. Когда CI выкладывает новую сборку, он ставит
Helm-параметр Application `image.tags.<образ>` в `latest@sha256:<digest>`: манифесты
меняются, Argo CD синхронизирует, поды переходят на новые образы, а хук `generator-init`
сразу пересобирает сайт новым генератором. В git ничего не пишется (запись через API Argo CD).
Образы публичные, учётные данные реестра не нужны.

Установите контроллер Helm-чартом в неймспейс Argo CD (чарт ставит и CRD `ImageUpdater`),
затем его настройку — `deploy/argocd/image-updater.yaml` (ресурс `ImageUpdater`: три образа,
стратегия `digest`, платформы amd64 и arm64). В чарте 1.5.0 — v1.5.0; `image.tag` берёт
сборку v1.5.1 (тот же CRD; новее Go из-за CVE-2026-78659 и Argo CD 3.6.2), пока не вышел
чарт с ней:

```sh
helm repo add argo https://argoproj.github.io/argo-helm
helm repo update argo
helm upgrade --install argocd-image-updater argo/argocd-image-updater -n argocd --version 1.5.0 --set image.tag=v1.5.1
kubectl apply -n argocd -f deploy/argocd/image-updater.yaml
```

Реестр проверяется раз в 2 минуты. Что он сделал:

```sh
kubectl -n argocd logs deploy/argocd-image-updater-controller --since 10m | grep podcast-site
argocd app get podcast-site --show-params
```

Параметры живут в Application, не в git: повторный `kubectl apply` вашего
`deploy/local/argocd-application.yaml` их сбрасывает, и Image Updater ставит их заново за
пару минут.

## Если что-то не работает

См. [troubleshooting.ru.md](troubleshooting.ru.md): Helm, генератор, rclone, веб и сеть Gopher
по шагам.
