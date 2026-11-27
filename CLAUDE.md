# podcast-site

Небольшой каталог подкастов, доступный по двум протоколам: **Gopher** и **HTTPS**.
Каталог состоит из M3U-плейлистов; сами аудиофайлы лежат на сервере.

## Окружение

- Сервер: Radxa на Intel N100 (x86_64), Linux.
- Разработка идёт в dev-контейнере (`.devcontainer/`, образ `mcr.microsoft.com/devcontainers/rust`),
  подключение из VS Code через Remote-SSH. Установка инструментов — `.devcontainer/post-create.sh`
  (Caddy из официального репозитория, Chromium, musl-цели Rust, cargo-audit/deny, GPG);
  свой Docker внутри — фича docker-in-docker (контейнер привилегированный).
- Аудио: rclone-монтирование `crypt:music/Podcast` → `/media/podcasts`,
  проброшено в контейнер **только на чтение**. Писать туда нельзя и не нужно.
- Порты в разработке: 7070 (Gopher), 8080 (HTTP). Порты <1024 в разработке не используем, root не нужен.
- Коммиты подписываются GPG-ключом `E1D456A80E71B7AF640F9B5A9D5A6C199F1B5404`. Секретный ключ —
  только на маке; агент пробрасывается мак → radxa (`RemoteForward` на `S.gpg-agent` и
  `S.gpg-agent.extra`) → контейнер (VS Code). Публичный ключ — `.devcontainer/signing-key.asc`,
  импортируется в `postCreateCommand`. Для разовых `ssh radxa` — `-o ClearAllForwardings=yes`,
  иначе проброс у сессии VS Code перехватывается.

- Репозиторий: https://github.com/extoor/podcast-site (публичный, remote `github`). Ветка `main`
  защищена ruleset'ом: только через pull request, подписанные коммиты, без force push, слияние
  squash/merge и только с зелёными проверками (`fmt, clippy, tests`, `cargo audit, cargo deny`,
  `inlined scripts, Caddyfile`, `no secrets in git`, `helm chart`, `generator`, `gopherd`, `web`).
  Порядок работы: ветка → push → `gh pr create` → `gh pr merge --squash --auto`.
  Remote `origin` (`git.tuhu.io`) — старая копия до переписывания истории, туда не пушим.

## Архитектура

Принцип: **одни данные — два вывода**. Каталог строится один раз генератором,
серверы отдают уже готовые файлы.

1. **podscan** (Rust, CLI) — сканирует `/media/podcasts`:
   - одна папка = один подкаст;
   - читает ID3-теги (название, исполнитель, альбом, длительность); если тегов нет —
     название берётся из имени файла;
   - пишет плейлисты M3U8 (UTF-8) со ссылками вида
     `https://<домен>/media/<путь-в-percent-encoding>`:
     `build/playlists/all.m3u8` (все эпизоды), `build/playlists/dir/<slug>.m3u8` (по папкам),
     `build/playlists/podcast/<slug>.m3u8` (по подкастам/шоу, `bez-podkasta.m3u8` — без подкаста);
     каждый плейлист пишется дважды с одним текстом: `.m3u8` (ссылки в вебе) и `.m3u` (ссылки
     в Gopher: клиенты и плееры на Спектруме знают только `.m3u`);
   - термины: **подкаст** (`Podcast`) — шоу, например Zebra; **папка** (`Folder`) — каталог
     в `/media/podcasts`, например KissFM. В `catalog.json` (формат 2): `podcasts`, `folders`,
     у эпизода — `podcast`;
   - эпизоды одного подкаста приводятся к единому виду: псевдонимы исполнителей из
     `[[artist]]` в `podcasts.toml` (опечатки), неверная граница исполнитель/название
     («MORDAX» + «Bastards ZEBRA») заменяется самой частой парой, регистр и пунктуация —
     к самому частому написанию;
   - тег названия без тега исполнителя не считается тегом (там бывают коды станции вроде
     `2015-075 PARTYFON RADIO`): исполнитель и название тогда берутся из имени файла;
   - подкаст = тег альбома (TALB), исправленный правилами из `podcasts.toml` (варианты
     написания и префиксы имён файлов; префикс файла важнее тега). Названия, отличающиеся
     только регистром/пунктуацией, сливаются автоматически;
   - пишет `build/catalog.json` — источник для генерации страниц.
   - файлы целиком не читаем (через rclone это означает скачивание): за одно открытие
     читается ID3-тег и 16 КБ после него (`podscan/src/mpeg.rs`). Из первого MPEG-фрейма —
     версия, layer, битрейт, CBR/VBR, частота, каналы (`audio` в `catalog.json`);
     длительность — из Xing/Info/VBRI или по размеру для CBR, иначе из TLEN.
2. **Генератор `sitegen`** (Rust) — из `catalog.json` строит `build/gopher/**/gophermap`
   и `build/www/**/index.html` (`index.html`, `podcast/<slug>/` — по подкастам,
   `dir/<slug>/` — по папкам; те же пути, что в Gopher). Языки: английский — основной (корень),
   русский и украинский — под `/ru/` и `/ua/`; тексты — таблица `LANGS` в `sitegen/src/html.rs`,
   переключатель `EN | UA | RU` (справа в строке «◄ all podcasts») ведёт на ту же страницу;
   `lang="uk"` (ISO-код языка), путь и подпись — `ua`; Gopher — на русском.
   Caddy: `Referrer-Policy same-origin` — по referrer keynav выделяет подкаст, из которого
   вернулись. Ссылки в HTML относительные
   (`/media/…`, `/playlists/…`), страницы работают на любом хосте.
   HTML в ретро-стиле: всё оформление — только текстовыми символами (без картинок и SVG).
   Заголовок «KISS FM / PODCASTS» — пиксельный шрифт из блоков `█` с контурами из
   box-drawing символов и радужным градиентом (в Gopher — тот же баннер без цвета).
   Внешние CSS/JS/шрифты — только с CDN (Google Fonts, cdnjs/jsDelivr), локальных
   статических файлов нет; свои стили и скрипты — inline в странице.
   Исключение — иконки сайта: `sitegen` сам рисует «FM» шрифтом баннера с радугой и тенью
   (`sitegen/src/icon.rs`, свой PNG/ICO без зависимостей): `favicon.ico` (16/32/48),
   `icon-192.png`, `apple-touch-icon.png` (180, с полями под скругление); CSP — `img-src 'self'`.
   На страницах подкастов и папок — текстовый плеер `web/textplayer/` (окно в стиле DOS
   по центру, рамки из box-drawing, анализатор спектра на Web Audio; клавиши Spc/P, ←→ — 10 с,
   ↑↓ — минута, S, +-, A — показать/спрятать анализатор (cookie `tp_spectrum`),
   закрыть — Esc или Backspace). Это отдельный переиспользуемый модуль без зависимостей
   и без знания о сайте (см. его README); генератор встраивает его
   в страницу и вызывает `TextPlayer.attach(...)`. В файле не должно быть закрывающего
   тега script. Без JS ссылки ведут прямо на MP3.
   Навигация с клавиатуры — `web/keynav/` (тоже встраивается): ↑↓/Home/End — выбор в списке,
   Enter — открыть подкаст или запустить плеер, Backspace — на главную; пока открыт плеер,
   клавиши его.
3. **Gopher-сервер** — свой, на Rust (tokio). Отдаёт `build/gopher`,
   плейлисты и, при необходимости, аудио.
4. **HTTPS** — Caddy, конфиг `caddy/Caddyfile`: отдаёт `build/www`, плейлисты и аудио из
   `/media/podcasts` (под `/media/` — только `*.mp3`: в библиотеке есть чужой `index.html`).
   CSP разрешает только inline-скрипты/стили и шрифты Google; админ-API выключен.
   Обязательно поддержка Range-запросов (перемотка в плеерах).
   Плейлисты `/playlists/*.m3u8` — с `Content-Disposition: attachment`: иначе Chrome и Safari
   открывают их встроенным HLS-плеером вместо скачивания (на страницах — атрибут `download`).

Сайт не для поисковиков: `sitegen` кладёт `robots.txt` (`Disallow: /`) в корень веба и Gopher,
в каждой странице — `<meta name="robots" content="noindex, nofollow, noarchive, ...">`, Caddy
шлёт тот же `X-Robots-Tag` на все ответы (MP3 и плейлисты тоже).

Генератор запускается по команде или по расписанию, не на каждый запрос.

## Требования к безопасности Gopher-сервера

Цель — отсутствие известных уязвимостей. Обязательно:

- селектор нормализуется и проверяется: никакого выхода за пределы корня
  (`..`, абсолютные пути, симлинки наружу, NUL-байты, управляющие символы);
- ограничение длины запроса (селектор ≤ 255 байт по RFC 1436), таймауты на чтение и запись;
- ограничение числа одновременных соединений;
- сервер только читает файлы, ничего не исполняет и не пишет;
- в продакшене — контейнер без root, read-only файловая система;
- `#![forbid(unsafe_code)]`, `cargo audit` и `cargo deny` перед каждым коммитом;
  минимум зависимостей.

## Gopher: напоминания

- Строка меню: `<тип><текст>\t<селектор>\t<хост>\t<порт>\r\n`, конец меню — `.\r\n`.
- Типы: `1` — меню, `0` — текст, `9` — бинарный файл, `i` — информационная строка,
  `h` — внешняя ссылка с селектором `URL:https://...`.
- Строки меню держим ≤ 70 символов, кодировка UTF-8, но только символы, которые есть в CP866
  (браузеры на Спектруме переводят текст в CP866, остальное показывают как `?`): `fit()` в
  `sitegen/src/gopher.rs` заменяет `—` → `-`, `…` → `...`, `◄` → `<` и т. п.

## Секреты: репозиторий публичный

- В git **никогда** не попадают пароли, ключи, токены, сертификаты с закрытым ключом,
  конфиги rclone (в них ключи шифрования `crypt:`), kubeconfig, `.env` и т. п. — ни в коде,
  ни в примерах, ни в тестах, ни в истории. Секреты живут вне репозитория: GitHub Actions
  secrets, Kubernetes Secret (создаётся отдельно, в чарте — только ссылка на него).
- `.gitignore` закрывает типичные файлы с секретами; публичный ключ подписи
  `.devcontainer/signing-key.asc` — исключение, он нужен.
- В CI работает gitleaks по всей истории; в настройках GitHub включить secret scanning и
  push protection (бесплатно для публичных репозиториев).
- Если секрет всё же попал в коммит — **сначала отозвать/сменить его**, потом чистить историю:
  удаление из истории не помогает, если коммит уже был запушен.
- Перед коммитом смотреть `git diff --cached` на предмет секретов; в примерах — заглушки
  (`<token>`, `example.org`).

## Соглашения

- Rust stable, `cargo fmt` и `cargo clippy -- -D warnings` без предупреждений.
- Ошибки — `anyhow` в бинарниках; паники в серверном коде недопустимы.
- Комментарии в коде и сообщения коммитов — **на английском**.
- README — на двух языках: `README.md` на английском, рядом `README.ru.md` на русском.
  Вверху каждого — переключатель языка: `**English** | [Русский](README.ru.md)` и
  `[English](README.md) | **Русский**`. Версии держим синхронными: правка одной —
  правка обеих.
- Сообщения программ пользователю (вывод CLI, ошибки) — на русском.
- `/target` и `/build` в `.gitignore`.

## Дорожная карта

1. [x] Dev-контейнер.
2. [x] podscan: сканирование, ID3, M3U8, catalog.json. Проверено на реальных файлах
       (KissFM, 291 MP3): читается ~9 КБ на файл, скан ~8 с через rclone.
3. [x] Генератор `sitegen`: gophermap и HTML (`cargo run -p sitegen -- --gopher-host <dev-host>`;
       хост и порт Gopher — `--gopher-host`, `--gopher-port`).
   Dev-запуск в контейнере (порты 7070 и 8080 опубликованы и на radxa, `appPort`):
   `cargo run -p podscan -- --base-url http://<dev-host>:8080`, затем sitegen, затем
   `cargo run -p gopherd -- --listen 0.0.0.0:7070` и `caddy run --config caddy/Caddyfile --adapter caddyfile`.
4. [x] Gopher-сервер `gopherd` (tokio): селекторы `/`, `/podcast/<slug>/`, `/dir/<slug>/`,
       `/playlists/…`, `/media/…`; запуск: `cargo run -p gopherd -- --listen 127.0.0.1:7070`.
       Тесты атак — `gopherd/tests/server.rs`.
   CI — GitHub Actions (`.github/workflows/`): `ci.yml` — fmt, clippy, тесты, cargo audit/deny,
   JS-модули, Caddyfile, gitleaks; `images.yml` — образы amd64+arm64 в GHCR
   (`ghcr.io/<owner>/<repo>/{generator,gopherd,web}`) из main и тегов `v*`, PR — только сборка,
   еженедельная пересборка (свежий Caddy). Dependabot — actions, cargo, базовые образы.
   Docker в dev-контейнере — для локальной проверки сборки.
5. [~] HTTPS через Caddy с Range-запросами: `caddy/Caddyfile` готов и проверен в dev (Range,
       attachment для плейлистов, CSP); в кластере TLS делают Traefik и cert-manager.
       Caddy всегда последней версии: в dev-контейнере — из официального apt-репозитория Caddy,
       в образе `web` — `caddy:2-alpine` со сборкой `--pull` и регулярной пересборкой.
6. [~] Docker-образы (GitHub Actions + GHCR, см. выше): `docker/Dockerfile`, цели `generator` (`sitegen --scan`), `gopherd`, `web`
       (Caddy + `caddy/Caddyfile`, без file capability — иначе не стартует при drop ALL);
       multi-arch amd64 + arm64: Rust-этап на `$BUILDPLATFORM` кросс-компилирует статический
       musl через `rust-lld` (без Zig и C-тулчейна), рантайм `distroless/static:nonroot`.
       Все образы rootless: UID/GID 65532 числом (иначе runAsNonRoot в Kubernetes не проверит),
       без capabilities, корневая ФС только на чтение. В dev-контейнере свой Docker
       (фича docker-in-docker) для `docker buildx`. Проверка живости — `gopherd --check <адрес>`.
7. [~] Kubernetes (radxa — только разработка, работает всё в кластере). Helm-чарт
       `deploy/helm/podcast-site` готов (проверка в CI — job `helm chart`: lint + kubeconform);
       осталось — первый деплой. Решено:
       - деплой — Helm-чарт в репозитории, ArgoCD (`deploy/argocd/application.yaml`), по желанию
         kustomize поверх чарта (`deploy/kustomize/`, `--enable-helm`); образы — GHCR;
       - домен — только `domain` в values (+ `extraDomains`): из него Certificate, IngressRoute,
         base URL плейлистов и `gopher.host` по умолчанию; нигде не дублируется;
       - аудио — rclone CSI-драйвер veloxpack (`rclone.csi.veloxpack.io`, проверен в кластере):
         PV `crypt:music/Podcasts`, ReadOnlyMany, конфиг — Secret с ключом `configData` (именно
         так, не `rclone.conf`): секции `[crypt]` и S3 под ней; fsGroup read-only тома kubelet
         драйверу не передаёт — чарт сам добавляет `allow-other`, `uid`/`gid` из podSecurityContext;
       - собранный сайт — PVC RWO, `site.storageClass` (по умолчанию `openebs-hostpath`) —
         одна нода: generator, gopherd и web на ней;
       - веб — Service + Traefik IngressRoute (`web` — порт 80, только
         middleware redirect-scheme из ns traefik; `websecure` — без middleware)
         + cert-manager Certificate (ClusterIssuer
         zerossl-production), Caddy внутри на 8080;
       - Gopher — Multus macvlan со своим IP в DMZ (аннотация `k8s.v1.cni.cncf.io/networks`,
         сеть `macvlan-dmz-static`, интерфейс `dmz`), gopherd слушает 70 без root через sysctl
         `net.ipv4.ip_unprivileged_port_start=0`; в меню — то же имя, что у веба, порт 70:
         маршрутизатор переадресует 70 публичного адреса на IP пода;
       - домен — `fm.tuhu.io` (веб и Gopher). Реальные значения (IP пода и т. п.) — не в git:
         `deploy/local/` в `.gitignore`;
       - Pod Security «restricted»: runAsNonRoot, readOnlyRootFilesystem, drop ALL, seccomp.

## Открытые вопросы

- ~~Домен сайта~~ — `fm.tuhu.io`, один для веба и Gopher.
- ~~Есть ли в каталоге не-MP3 файлы~~ — нет (только MP3 и один `index.html`), остаёмся на `id3`.
- ~~Длительность~~ — у файлов KissFM TLEN мусорный (≈ 27 ч у всех), теперь она берётся из
  MPEG-заголовка: известна у всех 291 файла (все MPEG-1 Layer III CBR, 192–320 кбит/с).
  Скан ~10 с через rclone.
- 191 из 291 файла без тега названия — исполнитель и название разбираются из имени файла
  (`podscan/src/filename.rs`): подкаст ищется в словах имени, остальное — исполнитель,
  написание исполнителя уточняется по тегам других файлов. Открыто: добавлять ли дату
  выпуска в название (сейчас в плейлисте подкаста много одинаковых строк).
- ~~Аудио по Gopher~~ — по Gopher (тип 9, `/media/…`); ссылок на HTTPS у эпизодов в меню нет
  (в корневом меню осталась одна `h`-ссылка «Веб-версия»).
