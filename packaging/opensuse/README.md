# openSUSE e OBS

Este diretório contém a receita RPM do Lyrnova. Nada é enviado ao Open Build
Service automaticamente.

O OBS executa o build sem acesso ao npm ou ao crates.io. Por isso o preparador
local produz dois arquivos auditáveis:

- `lyrnova-0.1.0.tar.zst`: código, documentação e frontend já compilado a partir
  do `package-lock.json`;
- `lyrnova-vendor-0.1.0.tar.zst`: crates resolvidas estritamente pelo
  `Cargo.lock`, além de `.cargo/config.toml` apontando para o vendor local.

Gere as fontes com:

```bash
./scripts/make-obs-sources.sh
```

Os arquivos ficam em `packaging/output/obs/`. Revise checksums e conteúdo antes
de qualquer upload. Para um teste local, copie a receita e os dois tarballs para
um package checkout do `osc` e execute `osc build`.

Dependências de sistema seguem os pré-requisitos Linux do Tauri 2. No openSUSE,
o pacote `webkit2gtk3-devel` fornece a API WebKitGTK 4.1.
