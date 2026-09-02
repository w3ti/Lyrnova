#!/usr/bin/env bash
set -euo pipefail

project_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$project_root/src-tauri/Cargo.toml" | head -n 1)
if [[ -z "$version" ]]; then
  echo "Não foi possível determinar a versão do Lyrnova." >&2
  exit 1
fi

output_dir="$project_root/packaging/output/obs"
work_dir=$(mktemp -d /tmp/lyrnova-obs.XXXXXX)
trap 'rm -rf -- "$work_dir"' EXIT

npm ci --prefix "$project_root/ui"
npm run build --prefix "$project_root/ui"

source_root="$work_dir/lyrnova-$version"
mkdir -p "$source_root"
tar -C "$project_root" \
  --exclude=.git \
  --exclude=target \
  --exclude=ui/node_modules \
  --exclude=packaging/output \
  -cf - . | tar -C "$source_root" -xf -

mkdir -p "$source_root/.cargo" "$work_dir/vendor"
(
  cd "$work_dir"
  cargo vendor --manifest-path "$project_root/Cargo.toml" --locked --versioned-dirs vendor
) > "$source_root/.cargo/config.toml"

mkdir -p "$output_dir"
tar --sort=name --mtime='UTC 2026-09-01' --owner=0 --group=0 --numeric-owner \
  -C "$work_dir" -I 'zstd -19 -T0' -cf "$output_dir/lyrnova-$version.tar.zst" \
  "lyrnova-$version"
tar --sort=name --mtime='UTC 2026-09-01' --owner=0 --group=0 --numeric-owner \
  -C "$work_dir" -I 'zstd -19 -T0' -cf "$output_dir/lyrnova-vendor-$version.tar.zst" vendor

cp "$project_root/packaging/opensuse/lyrnova.spec" "$output_dir/"
cp "$project_root/packaging/opensuse/lyrnova.changes" "$output_dir/"
cp "$project_root/packaging/opensuse/_constraints" "$output_dir/"
(cd "$output_dir" && sha256sum "lyrnova-$version.tar.zst" "lyrnova-vendor-$version.tar.zst" > SHA256SUMS)

echo "Fontes OBS preparadas em $output_dir"
