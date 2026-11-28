**English** | [Русский](deploy.ru.md)

# Deploying to Kubernetes

The Helm chart `deploy/helm/podcast-site` deploys everything:

- the domain is written once, `domain` (plus optional `extraDomains`); the cert-manager
  `Certificate`, both Traefik `IngressRoute`s (`web`, port 80, redirects to HTTPS
  through the `redirect-scheme` middleware; `websecure` has none) and the playlists' base
  URL all come from it;
- `generator` is a CronJob (`generator.schedule`) plus a Job after every install/upgrade;
  the built site lives on a ReadWriteOnce PVC (`site.storageClass`, `openebs-hostpath` by
  default), so the chart keeps every pod of the release on one node (`podAffinity`). With a
  node-local class the site is down while that node is; with a replicated one (OpenEBS
  Mayastor such as `mayastor-thin-r3`, Ceph RBD) the pods move to another node together. Gopher
  needs its network interface on that node: limit the nodes with `nodeSelector`;
- the audio is a read-only PV of the rclone CSI driver
  ([veloxpack/csi-driver-rclone](https://github.com/veloxpack/csi-driver-rclone),
  `crypt:music/Podcasts`); its configuration (key `configData`, the whole `rclone.conf`) is
  a Secret made outside the chart (`media.csi.nodePublishSecretRef`), or use your own
  claim with `media.type: existingClaim`. `helm install`/`upgrade` stops if the Secret or its
  `configData` key is missing. Nothing is cached on the node's disk: the chart pins
  `vfs-cache-mode=off`, so rclone keeps only directory listings and read buffers in memory;
- `gopherd` gets its own address through Multus (`gopher.network`) and listens on
  port 70 without root (the pod sysctl `net.ipv4.ip_unprivileged_port_start=0`); menus
  point to `gopher.host` (default: `domain`, the same name as the web);
- every pod passes Pod Security "restricted"; `networkPolicy.enabled` forbids egress and lets
  only Traefik reach the web pod.

## What the cluster needs

- Traefik with its CRDs (`traefik.io/v1alpha1`), entry points `web` and `websecure`, and the
  `redirect-scheme` middleware in namespace `traefik` (`ingressRoute.*` in the values);
- cert-manager and an issuer (`certificate.issuerRef`, default ClusterIssuer
  `zerossl-production`);
- the rclone CSI driver [veloxpack/csi-driver-rclone](https://github.com/veloxpack/csi-driver-rclone);
- a StorageClass for the built site (`site.storageClass`, default `openebs-hostpath`);
- for Gopher: Multus with the macvlan and static CNI plugins (the chart makes the
  NetworkAttachmentDefinition), or `gopher.enabled: false`.

## 1. Your values, outside git

The repository is public: the real domain and internal addresses go to `deploy/local/`,
which is in `.gitignore`. For example `deploy/local/values.yaml`:

```yaml
domain: podcasts.example.org
gopher:
  network:                 # the pod's own address (Multus macvlan)
    enabled: true
    master: eth0.100       # the node interface (here VLAN 100 on eth0)
    address: 192.0.2.41/24
    gateway: 192.0.2.1
web:
  trustedProxies: [<traefik-pod-cidr>]   # default private_ranges
```

All settings with comments are in `deploy/helm/podcast-site/values.yaml`.

## 2. The rclone Secret

The driver reads only the key `configData`: the whole `rclone.conf` with the `crypt`
section and the remote under it (only these two, not your other remotes). If the
configuration is password-protected, export an unencrypted copy with `rclone config show`.

```sh
kubectl create namespace podcasts
kubectl -n podcasts create secret generic podcast-site-rclone --from-file=configData=rclone.conf
```

## 3. Install

```sh
helm upgrade --install podcast-site deploy/helm/podcast-site -n podcasts -f deploy/local/values.yaml
kubectl -n podcasts get pods -w
```

Right after the install a Job builds the site (`podcast-site-generator-init`); the web and
Gopher pods become ready when it is done. Check:

```sh
kubectl -n podcasts get pods,certificate,ingressroute
kubectl -n podcasts exec deploy/podcast-site-gopherd -- /usr/local/bin/gopherd --check 127.0.0.1:70
kubectl -n podcasts logs deploy/podcast-site-web | tail -3
```

## Network

- DNS of `domain` points to the public address; ports 80 and 443 go to Traefik (the HTTP-01
  check of cert-manager needs port 80).
- Port 70 of the public address is forwarded to the Gopher pod's own address. Its answers must
  leave through the same interface, or the connection never opens. With
  `gopher.network.gateway` the chart routes `0.0.0.0/1` and `128.0.0.0/1` through it: more
  specific than the pod's default route on `eth0`, which Multus' `default-route` does not
  always replace (it did not with k3s/flannel). The cluster network stays on `eth0`. SNAT on the
  router works too. To look inside the distroless pod:
  `kubectl debug -it <pod> --image=nicolaka/netshoot --profile=netadmin -- ip route`.
- The visitor's address reaches the access log (`client_ip`) only if Traefik sees it
  (`externalTrafficPolicy: Local` on its Service, no SNAT before it) and its pods are in
  `web.trustedProxies`.

## Updates

The images are `latest` with `pullPolicy: Always`, rebuilt from `main` and weekly. To take
them and rebuild the site at once (otherwise it is rebuilt on `generator.schedule`):

```sh
helm upgrade --install podcast-site deploy/helm/podcast-site -n podcasts -f deploy/local/values.yaml
kubectl -n podcasts rollout restart deploy/podcast-site-web deploy/podcast-site-gopherd
kubectl -n podcasts create job --from=cronjob/podcast-site-generator podcast-site-build-now
```

With ArgoCD the first command is `argocd app sync podcast-site` (or wait for the automated
sync).

## ArgoCD

`deploy/argocd/application.yaml` is an example; keep your copy with the real values in
`deploy/local/` (the same values as in `deploy/local/values.yaml`). The application follows
`main`: push the changes there first. Coming from a Helm install: delete only the Helm release
record, ArgoCD takes over the running objects. Do not `helm uninstall`, it deletes the PVC with
the built site.

```sh
kubectl -n podcasts delete secret -l owner=helm,name=podcast-site
```

Then create the application, either with `kubectl`:

```sh
kubectl apply -f deploy/local/argocd-application.yaml
kubectl -n argocd get application podcast-site -w
```

or with the `argocd` CLI:

```sh
argocd login <argocd-host>
argocd app create -f deploy/local/argocd-application.yaml
argocd app wait podcast-site --health --sync --timeout 600
argocd app get podcast-site
```

The file turns on automated sync, so ArgoCD applies everything at once. To look at the diff
first, create the application with `--sync-policy none` (flags override the file):

```sh
argocd app create -f deploy/local/argocd-application.yaml --sync-policy none
argocd app diff podcast-site
argocd app sync podcast-site
argocd app set podcast-site --sync-policy automated --self-heal --auto-prune
```

Day to day:

```sh
argocd app get podcast-site --refresh
argocd app diff podcast-site
argocd app sync podcast-site
argocd app history podcast-site
```

- ArgoCD notices a new commit in `main` within about 3 minutes; `--refresh` makes it look now.
- The Helm hook `generator-init` runs as a PostSync hook: every sync rebuilds the site.
- The Secret check (`templates/checks.yaml`) needs the live cluster and is skipped in ArgoCD.
- The pods roll by themselves when the media mount changes (`checksum/storage`), not when the
  catalog is rebuilt.
- With `selfHeal` on, a manual change or `argocd app rollback` is reverted to what `main` says.

`deploy/kustomize/` is the same chart through kustomize (`kustomize build --enable-helm`) for
your own patches.

## When something goes wrong

See [troubleshooting.md](troubleshooting.md): Helm, the generator, rclone, the web and the
Gopher network, step by step.
