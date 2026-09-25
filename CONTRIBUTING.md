# Contribuindo com o Lyrnova

Obrigado por ajudar a construir o Lyrnova. O projeto está na fase de prova de
arquitetura; mudanças pequenas, auditáveis e acompanhadas de testes são
preferidas.

## Preparação

No Linux, instale Rust stable, Node.js 24, GTK 3, WebKitGTK 4.1 e as
dependências de build do Tauri disponíveis na sua distribuição. Para executar
plugins externos, instale também o Bubblewrap (`bwrap`).

```bash
npm ci --prefix ui
npm run build --prefix ui
cargo test --workspace
cargo run -p lyrnova
```

Nenhum desses comandos precisa de conta, API key ou rede para usar o adapter
mock atual.

## Antes de enviar uma mudança

```bash
npm run check --prefix ui
npm run build --prefix ui
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Para validar o fluxo visual de diff e revisão de commit no Linux, execute após
o build do frontend, em uma sessão gráfica com PyGObject e WebKitGTK 4.1:

```bash
python3 ui/tests/git-review-smoke.py
```

Esse smoke abre uma janela temporária, usa IPC sintético e não modifica
repositórios do usuário. Os testes Rust de Git usam repositórios em diretórios
temporários para validar os efeitos reais de revisão e commit.

A [jornada nativa do MVP](tests/e2e/README.md) usa o binário Tauri real,
WebKitWebDriver e um perfil temporário. Ela valida edição, busca, Git, terminal,
Tasks, falha de plugin e reinício sem IA; o workflow guarda seu relatório e
artefatos de falha. Com `--rust-analyzer /caminho/do/binario`, a suíte também
exercita o servidor Rust real; o CI prepara uma release standalone com checksum
fixado. Consulte o guia para dependências e limites de cobertura.

O [guia de diagnósticos Rust](docs/plugins/rust-diagnostics.md) descreve a ativação
com revisão de permissões e as limitações desta primeira integração LSP.

## Regras de segurança

- não configure origens ou recursos remotos no frontend;
- não crie command genérico para executar shell, abrir paths ou ler o
  filesystem;
- não permita que adapter, modelo ou AGENTS.md ampliem a própria autoridade;
- não registre tokens, secrets, prompts, conteúdo de arquivo ou environment;
- alterações de ferramenta, sandbox ou permissão exigem testes negativos e
  atualização do threat model;
- nunca inclua credenciais reais em fixture, captura ou issue.

## Arquitetura e interface

Leia os ADRs em `docs/architecture`, o threat model em `docs/security` e o
design system antes de alterar a fundação. Providers ficam atrás do protocolo
interno e efeitos passam pelo núcleo Rust. Estados locais devem funcionar com
teclado, movimento reduzido e janelas compactas.

## Commits e pull requests

- descreva o problema e a decisão, não apenas os arquivos alterados;
- vincule a issue correspondente;
- indique plataformas e runtimes testados;
- inclua teste ou explique por que não é possível automatizá-lo;
- não publique pacotes nem releases a partir de uma pull request.
