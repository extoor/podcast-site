# podcast-site

[English](README.md) | **Русский**

Небольшой каталог подкастов, доступный сразу по двум протоколам: **Gopher** и **HTTP(S)**.
Каталог строится из папки с MP3-файлами: один раз сканируем, получаем готовые меню,
страницы и плейлисты M3U8, а серверы их только отдают.

- **Веб:** страницы в ретро-стиле, нарисованные только текстовыми символами (пиксельный
  баннер из `█`, рамки из символов псевдографики, без картинок), аудиоплеер в стиле DOS
  прямо на странице, полное управление с клавиатуры.
- **Gopher:** тот же каталог в виде меню с тем же баннером; эпизоды и плейлисты
  (`.m3u`) скачиваются по Gopher. В тексте меню только символы CP866, поэтому он
  правильно показывается и в Gopher-браузерах на ZX Spectrum.
- **Плейлисты:** M3U8 на каждый подкаст, каждую папку и все эпизоды сразу — для VLC,
  foobar2000, mpv и подобных.

## Как это устроено

```
/media/podcasts ──podscan──► build/catalog.json ──sitegen──► build/gopher/**/gophermap ──► gopherd (Gopher)
   (MP3-файлы)        └────► build/playlists/*.m3u8      └──► build/www/**/index.html  ──► Caddy (HTTP/S)
                                                                                     аудио и плейлисты ───┘
```

Одни данные — два вывода. Генераторы запускаются по команде или по расписанию, а не на
каждый запрос; серверы только читают файлы.

| Часть | Что делает |
|-------|------------|
| [`podscan`](podscan/) | Сканирует медиатеку. За одно небольшое чтение берёт из каждого файла ID3-тег и первый MPEG-фрейм (версия, битрейт, CBR/VBR, частота, каналы, длительность) — файл целиком не скачивается. Если тегов нет, исполнитель и название разбираются из имени файла. Пишет `catalog.json` и плейлисты. |
| [`podcasts.toml`](podcasts.toml) | Правила, по которым эпизоды попадают в подкасты (шоу): варианты написания тега альбома и префиксы имён файлов. |
| [`sitegen`](sitegen/) | Строит из `catalog.json` меню Gopher и HTML-страницы. |
| [`gopherd`](gopherd/) | Gopher-сервер только на чтение (Rust, tokio). |
| [`caddy/Caddyfile`](caddy/Caddyfile) | Конфиг веб-сервера: страницы, плейлисты, аудио с Range-запросами (перемотка). |
| [`web/textplayer`](web/textplayer/) | Аудиоплеер в стиле DOS; отдельный модуль без зависимостей, годится и для других проектов. |
| [`web/keynav`](web/keynav/) | Навигация по страницам с клавиатуры: ↑↓ — выбор, Enter — открыть или играть, Backspace — назад. |

Термины: **подкаст** — шоу (например, Zebra), **папка** — каталог в медиатеке (например,
KissFM). В одной папке могут лежать эпизоды многих подкастов.

## Адреса

В вебе и в Gopher пути одинаковые:

| Путь | Веб | Gopher |
|------|-----|--------|
| `/` | главная | корневое меню |
| `/podcast/<slug>/` | эпизоды подкаста | меню |
| `/dir/<slug>/` | эпизоды папки | меню |
| `/ru/…`, `/ua/…` | те же страницы на русском и украинском (основная версия — английская; переключатель в углу каждой страницы) | — |
| `/playlists/…` | M3U8, скачивается файлом | M3U8 (тип 9) |
| `/media/<путь>` | MP3 (только `*.mp3`) | MP3 (тип 9) |

## Запуск

Разработка идёт в dev-контейнере (`.devcontainer/`, образ Rust): он ставит Caddy и
публикует на хосте порты 7070 (Gopher) и 8080 (HTTP). Медиатека смонтирована в
`/media/podcasts` только на чтение.

```bash
# 1. сканируем медиатеку; базовый адрес попадает в плейлисты
cargo run --release -p podscan -- --base-url http://<хост>:8080

# 2. строим меню и страницы; хост и порт Gopher попадают в каждую строку меню
cargo run --release -p sitegen -- --gopher-host <хост> --gopher-port 7070

# 3. серверы
cargo run --release -p gopherd -- --listen 0.0.0.0:7070
caddy run --config caddy/Caddyfile --adapter caddyfile
```

Дальше: `http://<хост>:8080/` в браузере и `gopher://<хост>:7070/` в Gopher-клиенте
(или `curl gopher://<хост>:7070/1/`). Параметры каждой программы — по `--help`.

В продакшене Caddy берёт из окружения `SITE_ADDRESS` (домен; сертификат он получит сам),
`BUILD_DIR` и `MEDIA_DIR`.

## Образы контейнеров

В продакшене всё работает в Kubernetes (dev-сервер — только для разработки).
`docker/Dockerfile` собирает три образа, каждый под `linux/amd64` и `linux/arm64` с одним тегом:

| Цель | Что это | Как запускается |
|------|---------|-----------------|
| `generator` | `sitegen --scan`: просканировать медиатеку и собрать сайт одной командой | CronJob |
| `gopherd` | Gopher-сервер | Deployment |
| `web` | Caddy с `caddy/Caddyfile`, обычный HTTP на 8080 (TLS — на Ingress) | Deployment |

```bash
docker buildx build --pull --platform linux/amd64,linux/arm64 -f docker/Dockerfile \
  --target gopherd -t <registry>/podcast-site/gopherd:<tag> --push .
```

Все образы rootless: работают от UID/GID 65532 (числом — чтобы `runAsNonRoot` в Kubernetes
мог это проверить), без capabilities и без записи в корневую ФС. Образ `web` берёт последний
Caddy 2.x на момент сборки (`caddy:2-alpine` с `--pull`) — его нужно регулярно пересобирать.

Бинарники Rust кросс-компилируются на родной архитектуре машины сборки в статические
musl-файлы (линкует `rust-lld` из тулчейна Rust; все зависимости на чистом Rust) — без
эмуляции и без C-тулчейна. Наши образы — `distroless/static:nonroot`: без shell, не от root,
несколько мегабайт. Генератор настраивается переменными `PODCAST_BASE_URL`, `GOPHER_HOST`,
`GOPHER_PORT`; `gopherd --check <адрес>` — проверка живости.

## Kubernetes

Всё разворачивает Helm-чарт `deploy/helm/podcast-site`:

- домен пишется один раз — `domain` (и при желании `extraDomains`); из него берутся
  `Certificate` cert-manager, оба `IngressRoute` Traefik (`web`, порт 80, перенаправляет на
  HTTPS через middleware `redirect-scheme`; у `websecure` middleware нет) и базовый адрес в плейлистах;
- `generator` — CronJob (`generator.schedule`) и Job после каждой установки/обновления;
  собранный сайт лежит на PVC (`site.storageClass`, по умолчанию `openebs-hostpath`, поэтому
  три пода на одной ноде);
- аудио — PV rclone CSI-драйвера только на чтение
  ([veloxpack/csi-driver-rclone](https://github.com/veloxpack/csi-driver-rclone),
  `crypt:music/Podcasts`); его конфигурация (ключ `configData`, весь `rclone.conf`) —
  Secret, создаваемый вне чарта (`media.csi.nodePublishSecretRef`), либо свой claim через
  `media.type: existingClaim`. `helm install`/`upgrade` останавливается, если Secret нет или в
  нём нет ключа `configData`;
- `gopherd` получает свой адрес через Multus (`gopher.podAnnotations`) и слушает порт 70
  без root (sysctl пода `net.ipv4.ip_unprivileged_port_start=0`); меню ссылаются на
  `gopher.host` (по умолчанию `domain` — то же имя, что у веба);
- все поды проходят Pod Security «restricted»; `networkPolicy.enabled` запрещает исходящие
  соединения и пускает к веб-поду только Traefik.

```sh
helm install podcast-site deploy/helm/podcast-site -n podcasts --create-namespace \
  --set domain=podcasts.example.org
```

Там же: `deploy/kustomize/` — тот же чарт через kustomize (`kustomize build --enable-helm`)
для своих патчей, и `deploy/argocd/application.yaml`.

## Непрерывная интеграция

GitHub Actions (`.github/workflows/`):

- `ci.yml` на каждый push и pull request: `cargo fmt`, `clippy -D warnings`, тесты,
  `cargo audit`, `cargo deny`, встраиваемые JS-модули, `Caddyfile` с последним Caddy и
  gitleaks по всей истории (в публичном репозитории не должно быть секретов) и Helm-чарт
  (`helm lint`, kubeconform со схемами cert-manager и Traefik).
- `images.yml`: три образа под amd64 + arm64 в `ghcr.io/<owner>/<repo>/{generator,gopherd,web}`
  из `main` и тегов `v*` (для pull request — только сборка); раз в неделю образы
  пересобираются с последним Caddy и свежими базовыми образами.
- Dependabot обновляет actions, crates и базовые образы.

## Безопасность

- `gopherd` проверяет каждый селектор до обращения к диску (никаких `..`, скрытых
  файлов, абсолютных путей, NUL и управляющих байтов), разрешает путь вместе с симлинками
  и отдаёт файл, только если он внутри своего корня. Длина запроса, таймауты на чтение и
  запись и число соединений ограничены. Сервер только читает файлы. Атаки покрыты
  интеграционными тестами (`gopherd/tests/server.rs`).
- Caddy отдаёт из медиатеки только MP3, присылает Content Security Policy, которая
  разрешает лишь собственный встроенный код страниц и шрифты Google, и работает с
  выключенным админ-API. Адрес посетителя (`X-Real-IP`, `X-Forwarded-For`) принимается только
  от доверенных прокси (`TRUSTED_PROXIES`, в чарте `web.trustedProxies`, по умолчанию
  `private_ranges`) и пишется в лог доступа как `client_ip`.
- Всё, что берётся из тегов и имён файлов, в HTML экранируется, а в меню Gopher
  очищается от табуляций и переводов строк.
- Без `unsafe`; `cargo audit` и `cargo deny` — перед каждым коммитом.

## Состояние

Работает: сканирование, плейлисты, меню и сервер Gopher, HTML-страницы, плеер, Caddy
в разработке, multi-arch rootless-образы в GHCR, Helm-чарт. Дальше: первый деплой в
кластер.

## Лицензия

[MIT](LICENSE).
