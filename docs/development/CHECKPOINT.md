# Checkpoint de desenvolvimento

Data: 2026-09-02
Commit-base: `18bee5b`
Estado: alterações locais validadas; aguardando a próxima etapa de desenvolvimento.

## Direção consolidada

- Lyrnova é um IDE desktop extensível, não um cliente Codex.
- Linguagens, frameworks, runtimes e provedores de IA são plugins opcionais.
- A instalação inicial não inclui nem ativa o Codex.
- O projeto usa Rust + Tauri 2 e uma interface inspirada no fluxo familiar do VS Code/Codex.
- Nada deve ser publicado, enviado ao GitHub ou ao OBS sem autorização explícita.

## Último trabalho concluído localmente

- Licença do código autoral migrada de MIT para `GPL-3.0-only`.
- Texto integral oficial da GNU GPL versão 3 instalado em `LICENSE`.
- Metadados Cargo, npm, AppStream e RPM atualizados, sem alterar as licenças das
  dependências.
- Documentação de escopo e README atualizados para refletir a nova licença.
- Área de configurações implementada.
- Configurações funcionais para fonte do editor, família tipográfica, tamanho das tabs,
  quebra de linha, espaços em branco, minimapa, ligaturas, rolagem suave, fonte do
  terminal e confirmação ao fechar arquivo modificado.
- Configurações são persistidas localmente e aplicadas ao Monaco em tempo real.
- Atalho `Ctrl+,`, entrada na paleta de comandos e botão de engrenagem adicionados.
- Sugestões futuras exibidas para tema/acessibilidade, atalhos, salvamento/formatação,
  perfis do terminal, Git e privacidade dos plugins.
- Tela vazia do editor mostra o ícone do Lyrnova quando todas as abas são fechadas.

## Validações já executadas em 2026-09-02

- `npm run check --prefix ui`
- `npm run build --prefix ui`
- `cargo fmt --all -- --check`
- `cargo check --workspace --offline`
- `cargo clippy --workspace --all-targets --offline -- -D warnings`
- `cargo test --workspace --offline` (48 testes aprovados e 1 teste de integração
  opcional ignorado por exigir Codex App Server local)
- `cargo metadata --offline --no-deps --format-version 1`
- `npm install --package-lock-only --ignore-scripts --offline --prefix ui`
- `appstreamcli validate --no-net`
- `xmllint --noout`
- `rpmspec -P`
- `git diff --check`

Todas passaram. O build mais recente foi aberto no Tauri; a área de configurações foi
acionada no WebView e também renderizada a partir do bundle local para inspeção em
1600 × 1000. Hierarquia, espaçamento, contraste, controles e convivência com o painel
do terminal foram revisados sem defeitos visuais bloqueantes.

## Ponto exato da licença

O código autoral usa a expressão SPDX `GPL-3.0-only`. O `LICENSE` tem 674 linhas e
corresponde byte a byte ao texto oficial da GNU GPL versão 3 instalado no sistema
(SHA-256 `3972dc9744f6499f0f9b2dbf76696f2ae7ad8af9b23dde66d6af86c9dfb36986`).
Referências a MIT, MPL, Apache, ISC e BSD que permanecem no lockfile pertencem a
dependências e não devem ser substituídas.

## Ao retomar

1. preservar a decisão `GPL-3.0-only` em novos metadados e templates;
2. continuar a fundação v0.1 pela plataforma de plugins e suas permissões;
3. manter providers de IA opcionais e ausentes da instalação inicial;
4. continuar sem push, publicação ou envio ao OBS sem autorização explícita.

## Estado do repositório

Há alterações locais não commitadas e vários arquivos novos ainda não rastreados. Eles
devem ser preservados. Nenhum push, pacote OBS ou release foi realizado neste ponto.
