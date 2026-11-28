[English](troubleshooting.md) | **Русский**

# Если не работает: что проверить

Шпаргалка по проверкам, которые пригодились при первом деплое. Команды рассчитаны на релиз
`podcast-site` в неймспейсе `podcasts`; поменяйте, если у вас иначе. Комментариев внутри
команд нет: в zsh без `interactive_comments` `#` ломает строку.

Начинать всегда с общей картины:

```sh
kubectl -n podcasts get pods,pvc,certificate,ingressroute
kubectl -n podcasts get events --sort-by=.lastTimestamp | tail -20
```

Под в `ContainerCreating` или `Pending` — смотрите `describe`, там причина (том, сеть,
планировщик):

```sh
kubectl -n podcasts describe pod -l app.kubernetes.io/component=gopherd | tail -20
```

## Helm

- `helm upgrade --install` падает с сообщением про Secret или `configData` — это проверка
  `templates/checks.yaml`; в тексте ошибки есть готовая команда для исправления (см. rclone ниже).
- Проверить, что чарт рендерится с вашими значениями, не трогая кластер:

```sh
helm template podcast-site deploy/helm/podcast-site -f deploy/local/values.yaml | less
```

## Сборка сайта (generator)

Веб и Gopher готовы, только когда генератор хотя бы раз собрал сайт.

```sh
kubectl -n podcasts logs job/podcast-site-generator-init
kubectl -n podcasts get cronjob,job
kubectl -n podcasts create job --from=cronjob/podcast-site-generator podcast-site-build-now
kubectl -n podcasts logs -f job/podcast-site-build-now
```

В конце лога — строка `Готово: … страниц HTML …`. Версию собранного сайта видно внизу
страницы (`rev <коммит>`) и в корневом меню Gopher (`Версия:`): если там старый коммит — образ
не обновился (`rollout restart` и пересборка, см. «Обновление» в [deploy.ru.md](deploy.ru.md)).

## rclone (аудио)

**1. Ключ Secret.** Драйвер читает только `configData`:

```sh
kubectl -n podcasts get secret podcast-site-rclone -o go-template='{{range $k,$v := .data}}{{$k}}{{"\n"}}{{end}}'
```

Должно быть ровно `configData`. Если там `rclone.conf` — пересоздать:

```sh
kubectl -n podcasts delete secret podcast-site-rclone
kubectl -n podcasts create secret generic podcast-site-rclone --from-file=configData=rclone.conf
kubectl -n podcasts delete pod -l app.kubernetes.io/instance=podcast-site
```

**2. Секции конфигурации** (без ключей — только имена и типы):

```sh
kubectl -n podcasts get secret podcast-site-rclone -o jsonpath='{.data.configData}' | base64 -d | grep -E '^\[|^type|^remote'
```

Нужны секция `[crypt]` с `remote = <s3-remote>:<bucket>` и сама секция S3-remote. Если
конфигурация зашифрована паролем, выгрузите открытую копию: `rclone config show crypt` и
`rclone config show <s3-remote>`.

**3. Лог драйвера на ноде пода** — тут видны ошибки монтирования и расшифровки:

```sh
NODE=$(kubectl -n podcasts get pod -l app.kubernetes.io/component=web -o jsonpath='{.items[0].spec.nodeName}')
POD=$(kubectl get pods -A -l app=csi-rclone-node --field-selector spec.nodeName=$NODE -o jsonpath='{.items[0].metadata.namespace} {.items[0].metadata.name}')
kubectl -n ${POD% *} logs ${POD#* } -c rclone --since 15m | grep -iE 'error|fail|crypt|NodePublish' | tail -40
```

**4. Тот же конфиг вручную** — разовый под с rclone, чтобы увидеть, что лежит по пути
(`remotePath` в values должен совпасть, с точностью до регистра и буквы `s`):

```sh
kubectl -n podcasts apply -f - <<'EOF'
apiVersion: v1
kind: Pod
metadata: { name: rclone-cli }
spec:
  restartPolicy: Never
  securityContext:
    runAsNonRoot: true
    runAsUser: 65532
    seccompProfile: { type: RuntimeDefault }
  containers:
    - name: rclone
      image: rclone/rclone:latest
      command: [sleep, "3600"]
      env:
        - { name: RCLONE_CONFIG, value: /cfg/rclone.conf }
        - { name: HOME, value: /tmp }
      securityContext:
        allowPrivilegeEscalation: false
        capabilities: { drop: [ALL] }
      volumeMounts:
        - { name: cfg, mountPath: /cfg/rclone.conf, subPath: configData, readOnly: true }
  volumes:
    - name: cfg
      secret: { secretName: podcast-site-rclone }
EOF
kubectl -n podcasts wait --for=condition=Ready pod/rclone-cli
kubectl -n podcasts exec rclone-cli -- rclone lsd crypt:
kubectl -n podcasts exec rclone-cli -- rclone lsd crypt:music
kubectl -n podcasts exec rclone-cli -- rclone lsf crypt:music/Podcasts
kubectl -n podcasts delete pod rclone-cli --now
```

Пусто в `crypt:`, хотя в бакете есть данные, — неверные пароли crypt или `remote` указывает
не туда. Ошибка доступа — проблема в S3-секции.

**5. Типичные симптомы:**

| Признак | Что делать |
|---|---|
| `FailedMount … either configData or remoteType must be provided` | Ключ Secret не `configData` — шаг 1. |
| `/media` пустой | Неверный `media.csi.volumeAttributes.remotePath` или ключи crypt — шаги 2 и 4. |
| `/media`: `Permission denied` | Том без `allow-other`/`uid`/`gid` (чарт добавляет их сам) — проверьте `podSecurityContext` и `media.csi.mountOptions`. |
| PVC `Pending`, `volume … already bound to a different claim` | PV остался `Released` после удаления PVC: `kubectl patch pv <pv> --type json -p '[{"op":"remove","path":"/spec/claimRef/uid"}]'` |

Изменения Secret подхватываются только при новом монтировании — после правки удалите поды.

## Веб (HTTPS)

```sh
kubectl -n podcasts describe certificate
kubectl -n podcasts get challenge
curl -sI https://<домен>/
curl -sI -r 0-99 https://<домен>/media/<файл>.mp3
curl -sI https://<домен>/playlists/all.m3u8
kubectl -n podcasts logs deploy/podcast-site-web | tail -5
kubectl -n podcasts exec deploy/podcast-site-web -- wget -qO- http://127.0.0.1:8081/health
```

Последняя — проверка готовности: `ok`, когда сайт собран, до того — ошибка (503). Порт 8081
внутренний, его нет в Service.

- Сертификат не выпускается — почти всегда DNS или порт 80 не доходит до Traefik (HTTP-01).
- На Range-запрос должен прийти `206 Partial Content` (иначе в плеерах не работает перемотка),
  у плейлиста — `Content-Disposition: attachment`.
- Первое поле строки лога Caddy — адрес посетителя. Если там адрес Traefik или маршрутизатора:
  поды Traefik не входят в `web.trustedProxies`, либо Traefik сам не видит адрес
  (`externalTrafficPolicy: Local`, без SNAT перед ним).

## Gopher

**1. Сервер жив** (та же проверка, что у проб; работает только изнутри пода, с любого
другого адреса `/health` — несуществующий путь):

```sh
kubectl -n podcasts exec deploy/podcast-site-gopherd -- /usr/local/bin/gopherd --check 127.0.0.1:70
kubectl -n podcasts logs -f deploy/podcast-site-gopherd
```

**2. Снаружи** (с машины вне кластера). Если не отвечает, но шаг 1 в порядке — дело в сети.

```sh
curl -v --max-time 10 gopher://<домен>/1/
```

Ещё вариант — через веб-шлюз: `https://gopher.floodgap.com/gopher/gw?a=gopher%3A%2F%2F<домен>%2F`.

**3. Второй интерфейс пода.** Multus пишет итог в аннотацию — должен быть интерфейс `net1`
с вашим адресом:

```sh
kubectl -n podcasts get pod -l app.kubernetes.io/component=gopherd -o jsonpath='{.items[0].metadata.annotations.k8s\.v1\.cni\.cncf\.io/network-status}'
kubectl -n podcasts get network-attachment-definitions -o yaml
```

**4. Маршруты и пакеты.** Образ distroless — без шелла, поэтому смотрим отладочным контейнером:

```sh
POD=$(kubectl -n podcasts get pod -l app.kubernetes.io/component=gopherd -o name | head -1)
kubectl -n podcasts debug -it $POD --image=nicolaka/netshoot --profile=netadmin -- bash
```

Внутри:

```sh
ip addr
ip route
ss -ltn
tcpdump -ni net1 tcp port 70
```

- `ip route`: должны быть `0.0.0.0/1` и `128.0.0.0/1` через шлюз из `gopher.network.gateway` на `dev net1`. Если есть
  только `default via … dev eth0` — ответы уходят в сеть кластера и соединение не
  устанавливается: проверьте `gopher.network.gateway`.
- `tcpdump` при `curl` снаружи: нет входящих SYN — переадресация порта 70 на маршрутизаторе
  или файрвол; SYN есть, а SYN-ACK уходит не туда — маршруты (выше).
- `ss -ltn`: gopherd слушает `0.0.0.0:70`.
- Hairpin NAT: проверка по публичному имени изнутри той же сети может не работать, даже когда
  снаружи всё в порядке, — проверяйте с внешней машины или через шлюз floodgap.

## Локально (dev-контейнер)

```sh
curl -s gopher://localhost:7070/1/ | head
curl -sI -r 0-99 http://localhost:8080/media/<файл>.mp3
caddy validate --config caddy/Caddyfile --adapter caddyfile
```

Страницы и меню берутся из `build/`: после правок кода — снова `cargo run -p sitegen -- --gopher-host <хост>`.
