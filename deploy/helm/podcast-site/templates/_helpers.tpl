{{/* Names and shared fragments. */}}

{{- define "ps.fullname" -}}
{{- if contains .Chart.Name .Release.Name -}}
{{- .Release.Name | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- printf "%s-%s" .Release.Name .Chart.Name | trunc 63 | trimSuffix "-" -}}
{{- end -}}
{{- end -}}

{{- define "ps.labels" -}}
app.kubernetes.io/name: {{ .Chart.Name }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/version: {{ .Values.image.tag | quote }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
helm.sh/chart: {{ printf "%s-%s" .Chart.Name .Chart.Version }}
{{- end -}}

{{/* Selector labels of one component: include "ps.selector" (list . "web") */}}
{{- define "ps.selector" -}}
{{- $ := index . 0 -}}
app.kubernetes.io/name: {{ $.Chart.Name }}
app.kubernetes.io/instance: {{ $.Release.Name }}
app.kubernetes.io/component: {{ index . 1 }}
{{- end -}}

{{/* The domain, checked once: every host name of the site is derived from it. */}}
{{- define "ps.domain" -}}
{{- required "values.domain: the site's domain is required" .Values.domain -}}
{{- end -}}

{{- define "ps.hosts" -}}
{{- $hosts := prepend .Values.extraDomains (include "ps.domain" .) -}}
{{- toJson (uniq $hosts) -}}
{{- end -}}

{{- define "ps.gopherHost" -}}
{{- default (include "ps.domain" .) .Values.gopher.host -}}
{{- end -}}

{{- define "ps.tlsSecret" -}}
{{- default (printf "%s-tls" (include "ps.fullname" .)) .Values.certificate.secretName -}}
{{- end -}}

{{- define "ps.image" -}}
{{- $ := index . 0 -}}
{{- printf "%s/%s:%s" $.Values.image.registry (index . 1) $.Values.image.tag -}}
{{- end -}}

{{- define "ps.mediaClaim" -}}
{{- if eq .Values.media.type "existingClaim" -}}
{{- required "values.media.existingClaim is required with media.type=existingClaim" .Values.media.existingClaim -}}
{{- else -}}
{{- printf "%s-media" (include "ps.fullname" .) -}}
{{- end -}}
{{- end -}}

{{/* Pod-level settings shared by all pods. */}}
{{- define "ps.podSpec" -}}
automountServiceAccountToken: false
securityContext:
  {{- toYaml .Values.podSecurityContext | nindent 2 }}
{{- with .Values.imagePullSecrets }}
imagePullSecrets:
  {{- toYaml . | nindent 2 }}
{{- end }}
{{- with .Values.nodeSelector }}
nodeSelector:
  {{- toYaml . | nindent 2 }}
{{- end }}
{{- with .Values.tolerations }}
tolerations:
  {{- toYaml . | nindent 2 }}
{{- end }}
{{- /*
A ReadWriteOnce site volume is attached to one node at a time, so every pod of the
release must run on the same node. A node-local class (openebs-hostpath) pins them by
itself; a replicated one (Mayastor, Ceph RBD) doesn't, so the pods follow each other: the
first one picks the node, the rest join it, and after a node failure the group moves
together. Added to .Values.affinity.
*/}}
{{- $aff := deepCopy (.Values.affinity | default dict) }}
{{- if eq .Values.site.accessMode "ReadWriteOnce" }}
{{- $pa := get $aff "podAffinity" | default dict }}
{{- $req := get $pa "requiredDuringSchedulingIgnoredDuringExecution" | default list }}
{{- $sel := dict "app.kubernetes.io/name" .Chart.Name "app.kubernetes.io/instance" .Release.Name }}
{{- $term := dict "labelSelector" (dict "matchLabels" $sel) "topologyKey" "kubernetes.io/hostname" }}
{{- $_ := set $pa "requiredDuringSchedulingIgnoredDuringExecution" (append $req $term) }}
{{- $_ := set $aff "podAffinity" $pa }}
{{- end }}
{{- with $aff }}
affinity:
  {{- toYaml . | nindent 2 }}
{{- end }}
{{- end -}}

{{/* The two data volumes; the site read-only unless "rw" is given. */}}
{{- define "ps.volumes" -}}
- name: site
  persistentVolumeClaim:
    claimName: {{ include "ps.fullname" (index . 0) }}-site
- name: media
  persistentVolumeClaim:
    claimName: {{ include "ps.mediaClaim" (index . 0) }}
    readOnly: true
{{- end -}}

{{- define "ps.mounts" -}}
- name: site
  mountPath: /site
  readOnly: {{ ne (index . 1) "rw" }}
- name: media
  mountPath: /media
  readOnly: true
{{- end -}}

{{/* The generator pod: a CronJob and the install-time Job share it. */}}
{{- define "ps.generatorPod" -}}
{{ include "ps.podSpec" . }}
restartPolicy: OnFailure
containers:
  - name: generator
    image: {{ include "ps.image" (list . "generator") }}
    imagePullPolicy: {{ .Values.image.pullPolicy }}
    env:
      - name: PODCAST_BASE_URL
        value: {{ printf "https://%s" (include "ps.domain" .) | quote }}
      - name: GOPHER_HOST
        value: {{ include "ps.gopherHost" . | quote }}
      - name: GOPHER_PORT
        value: {{ .Values.gopher.port | quote }}
    securityContext:
      {{- toYaml .Values.containerSecurityContext | nindent 6 }}
    resources:
      {{- toYaml .Values.generator.resources | nindent 6 }}
    volumeMounts:
      {{- include "ps.mounts" (list . "rw") | nindent 6 }}
      {{- if .Values.generator.rules }}
      - name: rules
        mountPath: /etc/podcast-site
        readOnly: true
      {{- end }}
volumes:
  {{- include "ps.volumes" (list .) | nindent 2 }}
  {{- if .Values.generator.rules }}
  - name: rules
    configMap:
      name: {{ include "ps.fullname" . }}-rules
  {{- end }}
{{- end -}}

{{/*
Pod annotation that changes with the volumes (the rendered storage.yaml: the media PV's
mount options, attributes and secret ref): a mount option only applies on a new mount, so
the long-running pods roll when it changes, with Helm and ArgoCD alike.
*/}}
{{- define "ps.storageChecksum" -}}
checksum/storage: {{ include (print .Template.BasePath "/storage.yaml") . | sha256sum }}
{{- end }}
