**English** | [Русский](troubleshooting.ru.md)

# When something doesn't work: what to check

A cheat sheet of the checks that helped during the first deployment. The commands assume
the release `podcast-site` in the namespace `podcasts`; change them if yours differ. There are
no comments inside the commands: in zsh without `interactive_comments` a `#` breaks the line.

Always start with the overall picture:

```sh
kubectl -n podcasts get pods,pvc,certificate,ingressroute
kubectl -n podcasts get events --sort-by=.lastTimestamp | tail -20
```

A pod stuck in `ContainerCreating` or `Pending`: `describe` shows the reason (volume, network,
scheduler):

```sh
kubectl -n podcasts describe pod -l app.kubernetes.io/component=gopherd | tail -20
```

## Helm

- `helm upgrade --install` fails with a message about the Secret or `configData`: that is the
  check in `templates/checks.yaml`; the error text contains the command that fixes it (see rclone
  below).
- Check that the chart renders with your values without touching the cluster:

```sh
helm template podcast-site deploy/helm/podcast-site -f deploy/local/values.yaml | less
```

## Site build (generator)

Web and Gopher become ready only after the generator has built the site at least once.

```sh
kubectl -n podcasts logs job/podcast-site-generator-init
kubectl -n podcasts get cronjob,job
kubectl -n podcasts create job --from=cronjob/podcast-site-generator podcast-site-build-now
kubectl -n podcasts logs -f job/podcast-site-build-now
```

The log ends with `Готово: … страниц HTML …`. The version of the built site is at the bottom of
every page (`rev <commit>`) and in the Gopher root menu (`Версия:`): an old commit there means
the image wasn't updated (`rollout restart` and a rebuild, see "Updates" in [deploy.md](deploy.md)).

## rclone (audio)

**1. The Secret key.** The driver reads only `configData`:

```sh
kubectl -n podcasts get secret podcast-site-rclone -o go-template='{{range $k,$v := .data}}{{$k}}{{"\n"}}{{end}}'
```

It must be exactly `configData`. If it says `rclone.conf`, recreate it:

```sh
kubectl -n podcasts delete secret podcast-site-rclone
kubectl -n podcasts create secret generic podcast-site-rclone --from-file=configData=rclone.conf
kubectl -n podcasts delete pod -l app.kubernetes.io/instance=podcast-site
```

**2. Config sections** (no keys, only names and types):

```sh
kubectl -n podcasts get secret podcast-site-rclone -o jsonpath='{.data.configData}' | base64 -d | grep -E '^\[|^type|^remote'
```

You need a `[crypt]` section with `remote = <s3-remote>:<bucket>` and the S3 remote's own
section. If the config is password-protected, export a plain copy: `rclone config show crypt`
and `rclone config show <s3-remote>`.

**3. The driver log on the pod's node** shows mount and decryption errors:

```sh
NODE=$(kubectl -n podcasts get pod -l app.kubernetes.io/component=web -o jsonpath='{.items[0].spec.nodeName}')
POD=$(kubectl get pods -A -l app=csi-rclone-node --field-selector spec.nodeName=$NODE -o jsonpath='{.items[0].metadata.namespace} {.items[0].metadata.name}')
kubectl -n ${POD% *} logs ${POD#* } -c rclone --since 15m | grep -iE 'error|fail|crypt|NodePublish' | tail -40
```

**4. The same config by hand.** A one-off rclone pod shows what is at the path
(`remotePath` in values must match it exactly, case and a trailing `s` included):

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

`crypt:` is empty although the bucket has data: wrong crypt passwords, or `remote` points
elsewhere. An access error: the S3 section is wrong.

**5. Typical symptoms:**

| Symptom | What to do |
|---|---|
| `FailedMount … either configData or remoteType must be provided` | The Secret key isn't `configData`: step 1. |
| `/media` is empty | Wrong `media.csi.volumeAttributes.remotePath` or crypt keys: steps 2 and 4. |
| `/media`: `Permission denied` | The volume lacks `allow-other`/`uid`/`gid` (the chart adds them): check `podSecurityContext` and `media.csi.mountOptions`. |
| PVC `Pending`, `volume … already bound to a different claim` | The PV stayed `Released` after its PVC was deleted: `kubectl patch pv <pv> --type json -p '[{"op":"remove","path":"/spec/claimRef/uid"}]'` |

A changed Secret is picked up only on a new mount: delete the pods after editing it.

## Web (HTTPS)

```sh
kubectl -n podcasts describe certificate
kubectl -n podcasts get challenge
curl -sI https://<domain>/
curl -sI -r 0-99 https://<domain>/media/<file>.mp3
curl -sI https://<domain>/playlists/all.m3u8
kubectl -n podcasts logs deploy/podcast-site-web | tail -5
```

- The certificate isn't issued: almost always DNS, or port 80 doesn't reach Traefik (HTTP-01).
- A Range request must get `206 Partial Content` (otherwise seeking in players breaks); the
  playlist must have `Content-Disposition: attachment`.
- `client_ip` in the Caddy log is the visitor's address. If it shows Traefik or the router
  instead: Traefik's pods aren't in `web.trustedProxies`, or Traefik itself doesn't see the
  address (`externalTrafficPolicy: Local`, no SNAT in front of it).

## Gopher

**1. The server is alive** (inside the pod and on its own address):

```sh
kubectl -n podcasts exec deploy/podcast-site-gopherd -- /usr/local/bin/gopherd --check 127.0.0.1:70
kubectl -n podcasts exec deploy/podcast-site-gopherd -- /usr/local/bin/gopherd --check <pod-address>:70
kubectl -n podcasts logs -f deploy/podcast-site-gopherd
```

**2. From outside** (a machine outside the cluster). No answer while step 1 is fine means the
network.

```sh
curl -v --max-time 10 gopher://<domain>/1/
```

Or through the web gateway: `https://gopher.floodgap.com/gopher/gw?a=gopher%3A%2F%2F<domain>%2F`.

**3. The pod's second interface.** Multus writes the result to an annotation: there must be an
interface `net1` with your address:

```sh
kubectl -n podcasts get pod -l app.kubernetes.io/component=gopherd -o jsonpath='{.items[0].metadata.annotations.k8s\.v1\.cni\.cncf\.io/network-status}'
kubectl -n podcasts get network-attachment-definitions -o yaml
```

**4. Routes and packets.** The image is distroless, with no shell, so use a debug container:

```sh
POD=$(kubectl -n podcasts get pod -l app.kubernetes.io/component=gopherd -o name | head -1)
kubectl -n podcasts debug -it $POD --image=nicolaka/netshoot --profile=netadmin -- bash
```

Inside:

```sh
ip addr
ip route
ss -ltn
tcpdump -ni net1 tcp port 70
```

- `ip route`: there must be `0.0.0.0/1` and `128.0.0.0/1` via the `gopher.network.gateway` on `dev net1`. With
  only `default via … dev eth0` the replies go into the cluster network and the connection
  never completes: check `gopher.network.gateway`.
- `tcpdump` during a `curl` from outside: no incoming SYN means the router's port 70 forward or
  a firewall; SYN arrives but the SYN-ACK leaves the wrong way means the routes (above).
- `ss -ltn`: gopherd listens on `0.0.0.0:70`.
- Hairpin NAT: a check by the public name from inside the same network may fail even when the
  outside works; test from an outside machine or through the floodgap gateway.

## Locally (dev container)

```sh
curl -s gopher://localhost:7070/1/ | head
curl -sI -r 0-99 http://localhost:8080/media/<file>.mp3
caddy validate --config caddy/Caddyfile --adapter caddyfile
```

Pages and menus come from `build/`: after code changes run `cargo run -p sitegen -- --gopher-host <host>` again.
