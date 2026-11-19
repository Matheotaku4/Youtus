#!/usr/bin/env bash
# Mesures M0 : lance le spike, échantillonne fps / ms-frame (CSV produit par
# l'app), CPU et RSS (/proc), puis remplit la ligne du tableau de verdict.
#
# Usage :
#   scripts/m0-measure.sh --label "1280×800" --seconds 30
#   scripts/m0-measure.sh --label "pause 10 s" --seconds 12 --pause 2
#   scripts/m0-measure.sh --label "1280×800" --csv mesures.csv --samples cpu.csv
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MANIFEST="$ROOT/spikes/m0-slint-mpv/Cargo.toml"
BIN="$ROOT/spikes/m0-slint-mpv/target/release/m0-slint-mpv"
VERDICT="$ROOT/docs/superpowers/spikes/2026-11-19-m0-verdict.md"

LABEL=""
RUN_SECONDS=30
VIDEO="/tmp/m0.mp4"
CSV=""
SAMPLES=""
RUN=1
PAUSE_SEC=""

usage() {
    cat <<'EOF'
Usage: scripts/m0-measure.sh --label "LIBELLÉ" [options]

  --label LIBELLÉ   ligne du tableau de verdict à remplir (obligatoire)
  --seconds N       durée de la mesure en secondes (défaut 30)
  --video CHEMIN    vidéo de test (défaut /tmp/m0.mp4)
  --pause N         met la lecture en pause automatiquement à N secondes
  --csv CHEMIN      réutilise un CSV de mesures (n'exécute pas l'application)
  --samples CHEMIN  réutilise un CSV d'échantillons CPU/RSS (avec --csv)
  --bin CHEMIN      binaire à lancer (défaut target/release/m0-slint-mpv)
  --verdict CHEMIN  fichier de verdict à mettre à jour
  -h, --help        cette aide

Le CSV de mesures est produit par l'application quand M0_METRICS est défini :
  t_s,fps,render_ms,position_s,duration_s,playing
Le CSV d'échantillons est produit par ce script :
  t_s,cpu_pct,rss_kb
EOF
}

while [ $# -gt 0 ]; do
    case "$1" in
        --label) LABEL="${2:-}"; shift 2 ;;
        --seconds) RUN_SECONDS="${2:-}"; shift 2 ;;
        --video) VIDEO="${2:-}"; shift 2 ;;
        --pause) PAUSE_SEC="${2:-}"; shift 2 ;;
        --csv) CSV="${2:-}"; RUN=0; shift 2 ;;
        --samples) SAMPLES="${2:-}"; shift 2 ;;
        --bin) BIN="${2:-}"; shift 2 ;;
        --verdict) VERDICT="${2:-}"; shift 2 ;;
        -h|--help) usage; exit 0 ;;
        *) echo "option inconnue : $1" >&2; usage; exit 2 ;;
    esac
done

[ -n "$LABEL" ] || { echo "erreur : --label est obligatoire" >&2; usage; exit 2; }
command -v python3 >/dev/null 2>&1 || { echo "erreur : python3 est requis" >&2; exit 2; }

if [ "$RUN" = "1" ]; then
    [ -f "$VIDEO" ] || { echo "erreur : vidéo introuvable : $VIDEO" >&2; exit 2; }
    cargo build --manifest-path "$MANIFEST" --release

    CSV="$(mktemp /tmp/m0-metrics.XXXXXX.csv)"
    SAMPLES="$(mktemp /tmp/m0-samples.XXXXXX.csv)"
    printf 't_s,cpu_pct,rss_kb\n' > "$SAMPLES"
    HZ="$(getconf CLK_TCK)"

    M0_METRICS="$CSV" M0_AUTOPAUSE_SEC="$PAUSE_SEC" "$BIN" "$VIDEO" &
    APP_PID=$!

    cleanup() {
        kill "$APP_PID" 2>/dev/null || true
        wait "$APP_PID" 2>/dev/null || true
    }
    trap cleanup EXIT

    echo "mesure « $LABEL » : ${RUN_SECONDS}s, binaire $BIN"
    prev_total=0
    end=$((SECONDS + RUN_SECONDS))
    while [ "${SECONDS}" -lt "$end" ] && kill -0 "$APP_PID" 2>/dev/null; do
        if [ -r "/proc/$APP_PID/stat" ]; then
            line="$(cat "/proc/$APP_PID/stat")"
            rest="${line##*)}"
            # shellcheck disable=SC2086
            set -- $rest
            # après la parenthèse : 1=state, 12=utime, 13=stime
            total=$(( ${12:-0} + ${13:-0} ))
            cpu=$(( (total - prev_total) * 100 / HZ ))
            prev_total=$total
            rss="$(awk '/^VmRSS:/{print $2}' "/proc/$APP_PID/status" 2>/dev/null || echo 0)"
            printf '%s,%s,%s\n' "$SECONDS" "$cpu" "${rss:-0}" >> "$SAMPLES"
        fi
        sleep 1
    done

    cleanup
    trap - EXIT
fi

[ -f "$CSV" ] || { echo "erreur : CSV de mesures introuvable : $CSV" >&2; exit 2; }
[ -f "$SAMPLES" ] || { echo "erreur : CSV d'échantillons introuvable : $SAMPLES" >&2; exit 2; }

python3 - "$CSV" "$SAMPLES" "$VERDICT" "$LABEL" <<'PY'
import sys

metrics_path, samples_path, verdict_path, label = sys.argv[1:5]

fps, ms = [], []
with open(metrics_path, encoding="utf-8") as fh:
    fh.readline()
    for line in fh:
        parts = line.strip().split(",")
        if len(parts) < 6:
            continue
        fps.append(float(parts[1]))
        ms.append(float(parts[2]))

cpu, rss = [], []
with open(samples_path, encoding="utf-8") as fh:
    fh.readline()
    for line in fh:
        parts = line.strip().split(",")
        if len(parts) < 3:
            continue
        cpu.append(float(parts[1]))
        rss.append(float(parts[2]))

if not fps:
    print(f"erreur : aucune mesure fps dans {metrics_path}", file=sys.stderr)
    sys.exit(2)

active = [value for value in fps if value > 0] or fps
fps_min = min(active)
fps_avg = sum(active) / len(active)
ms_avg = sum(ms) / len(ms)
cpu_avg = sum(cpu) / len(cpu) if cpu else 0.0
rss_peak_mb = max(rss) / 1024.0 if rss else 0.0

row = (
    f"| {label} | {fps_min:.0f} | {fps_avg:.0f} | {ms_avg:.1f} "
    f"| {cpu_avg:.0f} | {rss_peak_mb:.0f} |"
)

lines = open(verdict_path, encoding="utf-8").read().splitlines()
replaced = False
for index, line in enumerate(lines):
    if line.startswith(f"| {label} |"):
        lines[index] = row
        replaced = True
        break

if not replaced:
    for index, line in enumerate(lines):
        if line.strip() == "|---|---|---|---|---|---|":
            lines.insert(index + 1, row)
            replaced = True
            break

if not replaced:
    lines.append(row)
    print(f"avertissement : tableau introuvable, ligne ajoutée en fin de {verdict_path}")

with open(verdict_path, "w", encoding="utf-8") as fh:
    fh.write("\n".join(lines) + "\n")

print(f"« {label} » : {fps_min:.0f}–{fps_avg:.0f} fps, {ms_avg:.1f} ms/frame, "
      f"{cpu_avg:.0f} % CPU, {rss_peak_mb:.0f} Mo RSS")
print(f"verdict mis à jour : {verdict_path}")

if fps_min >= 55 and ms_avg <= 4.0 and cpu_avg <= 20.0:
    print("→ compatible d'un GO FemtoVG (à confirmer visuellement)")
else:
    print("→ sous les seuils GO : lire le verdict avant de conclure")
PY
