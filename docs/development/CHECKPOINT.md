# Checkpoint de desenvolvimento

Data: 2026-09-02
Commit-base: `fe3c314`
Estado: catálogo dinâmico de plugins externos implementado localmente; alterações ainda não commitadas.

## Direção consolidada

- Lyrnova é um IDE desktop extensível, não um cliente Codex.
- Linguagens, frameworks, runtimes e provedores de IA são plugins opcionais.
- A instalação inicial não inclui nem ativa o Codex.
- O projeto usa Rust + Tauri 2 e uma interface inspirada no fluxo familiar do VS Code/Codex.
- Nada deve ser publicado, enviado ao GitHub ou ao OBS sem autorização explícita.

## Último trabalho concluído localmente

- Contrato v1 de `plugin.json` implementado com schema JSON e validação Rust
  estrita.
- Catálogo hardcoded substituído por manifests embutidos e validados para Rust,
  Web Essentials e Codex.
- Capabilities, permissões, tipos, runtime e origem passaram a usar enums fechados.
- Estado de plugins migrado para v3 com concessões separadas; mudança de permissões
  desabilita o plugin até nova revisão.
- Instalação exige aprovação exata das permissões e operações sensíveis do adapter
  Codex verificam declaração e concessão.
- Manifests externos não podem se declarar embutidos; processos exigem entrypoint
  relativo, protocolo conhecido e `process_spawn`.
- Pacotes externos `.tar.zst` usam descritor SHA-256 separado do manifesto para
  evitar a circularidade de um arquivo declarar o próprio hash.
- Instalador local em duas fases implementado: staging/inspeção e instalação após
  aprovação exata das permissões.
- Extração limitada por tamanho comprimido, fluxo descomprimido, arquivo, soma e
  quantidade; traversal, paths não UTF-8, links, tipos especiais e duplicatas falham.
- Manifesto externo e entrypoint são revalidados no staging; falha ou abandono limpa
  temporários; instalação usa rename atômico, não substitui versão e nasce desabilitada
  e sem bit de execução.
- Instalações externas recebem recibo host-managed e SHA-256 determinístico da árvore,
  cobrindo paths, tipos, modos, tamanhos e conteúdo.
- O catálogo dinâmico redescobre e revalida pacotes a cada reload, escolhe a versão
  SemVer mais recente e impede que externos substituam IDs embutidos.
- Corrupção ou layout inválido falha fechado para todo o catálogo externo, removendo-o
  também do estado de autoridade em memória.
- Estado de plugins migrado para v4 com versões instaladas; upgrades removem grants e
  habilitação até nova revisão; aprovação externa pode ser persistida sem ativação.
- Alterações de instalação, remoção e habilitação só entram em memória depois de a
  persistência ser concluída.
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
- `cargo test --workspace --offline` (74 testes aprovados e 1 teste de integração
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
2. adicionar commands Tauri e interface para selecionar pacote, revisar manifesto e
   permissões e confirmar ou cancelar o staging;
3. implementar remoção física transacional de pacotes externos;
4. implementar download somente depois, com descritor vindo de catálogo confiável;
5. manter providers de IA opcionais e ausentes da instalação inicial;
6. continuar sem commit, push, publicação ou envio ao OBS sem autorização explícita.

## Estado do repositório

Há alterações locais não commitadas do catálogo dinâmico e um ADR novo ainda não
rastreado. Eles devem ser preservados. Nenhum push, pacote OBS ou release foi
realizado neste ponto.
