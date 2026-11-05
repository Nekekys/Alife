#!/usr/bin/env bash
# Развёртка по одному параметру на нескольких сидах.
#
# Проект требует добавлять по одной механике и после каждой сравнивать
# результат на нескольких сидах. Делать это руками — значит не делать.
#
#   scripts/sweep.sh attack_yield 0.0,0.4,0.8 --seeds 1,2,3 --steps 10000
#
# Запускать из корня репозитория. На Windows — через Git Bash.
set -euo pipefail

if [ $# -lt 2 ]; then
  sed -n '2,10p' "$0" | sed 's/^# \{0,1\}//'
  exit 2
fi

param="$1"
values="$2"
shift 2

config="configs/default.toml"
seeds="1,2,3"
steps=10000
outdir="runs/sweep"

while [ $# -gt 0 ]; do
  case "$1" in
    --config) config="$2"; shift 2 ;;
    --seeds)  seeds="$2";  shift 2 ;;
    --steps)  steps="$2";  shift 2 ;;
    --outdir) outdir="$2"; shift 2 ;;
    *) echo "неизвестный аргумент: $1" >&2; exit 2 ;;
  esac
done

bin=./target/release/alife
[ -x "$bin" ] || bin=./target/release/alife.exe
if [ ! -x "$bin" ]; then
  echo "сначала соберите: cargo build --release" >&2
  exit 1
fi
if ! grep -q "^${param} = " "$config"; then
  echo "в $config нет параметра '$param' на верхнем уровне своей секции" >&2
  exit 1
fi

mkdir -p "$outdir"

# Плотность считается от размера мира, а не от зашитого числа.
width=$(sed -n 's/^width = \([0-9]*\).*/\1/p' "$config" | head -1)
height=$(sed -n 's/^height = \([0-9]*\).*/\1/p' "$config" | head -1)
cells=$(( ${width:-256} * ${height:-256} ))

# Колонки CSV, которые сводим в таблицу.
#  2 численность, 11 поколение, 14 убито, 18 попаданий/атаку,
# 19 убийств/попадание, 21 животной пищи, 24 атак/особь, 25 рождений заблокировано
printf '%-14s %-5s %8s %8s %7s %7s %9s %8s %10s %7s\n' \
  "$param" сид особей плотн% покол убито поп/атаку наводка животн.пищи блокир

for v in ${values//,/ }; do
  cfg="$outdir/${param}_${v}.toml"
  sed "s/^${param} = .*/${param} = ${v}/" "$config" > "$cfg"
  for s in ${seeds//,/ }; do
    csv="$outdir/${param}_${v}_s${s}.csv"
    "$bin" run --config "$cfg" --steps "$steps" --every 500 --seed "$s" --out "$csv" >/dev/null 2>&1
    tail -1 "$csv" | awk -F, -v p="$v" -v s="$s" -v cells="$cells" '
      {
        occ = $2 / cells
        aim = occ > 0 ? $18 / occ : 0
        printf "%-14s %-5s %8d %7.2f%% %7d %7d %9.4f %8.2f %9.2f%% %7d\n",
               p, s, $2, occ*100, $11, $14, $18, aim, $21*100, $25
      }'
  done
done
